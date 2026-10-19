//! Хранилище: асинхронный API поверх PostgreSQL (sqlx).
//!
//! Реализованы методы:
//! * добавление мест, вещей, атрибутов и значений атрибутов;
//! * назначение/снятие атрибутов с вещи;
//! * списки мест, вещей, атрибутов и их значений;
//! * полная информация о вещи (местонахождение + атрибуты);
//! * перемещение вещи между местами.

use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Row};

use crate::error::{Result, StoreError};
use crate::models::{
    Attribute, AttributeValue, Item, ItemInfo, NewItem, NewPlace, Place,
};

/// Точка доступа к хранилищу.
#[derive(Debug, Clone)]
pub struct Store {
    pool: PgPool,
}

fn place_from_row(r: PgRow) -> Place {
    Place {
        id: r.get("id"),
        name: r.get("name"),
        description: r.get("description"),
    }
}

fn item_from_row(r: PgRow) -> Item {
    Item {
        id: r.get("id"),
        place_id: r.get("place_id"),
        name: r.get("name"),
        description: r.get("description"),
        photo: r.get("photo"),
    }
}

fn attr_value_from_row(r: PgRow) -> AttributeValue {
    AttributeValue {
        id: r.get("id"),
        attribute_id: r.get("attribute_id"),
        attribute_name: r.get("attribute_name"),
        value: r.get("value"),
    }
}

impl Store {
    /// Подключение к базе по строке соединения с пулом заданного размера.
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(database_url)
            .await
            .map_err(StoreError::from_db)?;
        Ok(Self { pool })
    }

    /// Создание хранилища из уже готового пула.
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Применить схему БД (schema.sql) к существующему подключению.
    pub async fn migrate(&self) -> Result<()> {
        let schema = include_str!("../schema.sql");
        // sqlx не выполняет несколько операторов одним prepared statement,
        // поэтому разбиваем файл на отдельные инструкции.
        for stmt in split_sql_statements(schema) {
            sqlx::query(stmt)
                .execute(&self.pool)
                .await
                .map_err(|e| {
                    let msg = format!(
                        "{} (statement: {})",
                        StoreError::from_db(e),
                        trim_for_msg(stmt)
                    );
                    StoreError::Database(sqlx::Error::Protocol(msg))
                })?;
        }
        Ok(())
    }

    // ============================ МЕСТА ============================

    /// Добавить место. Возвращает созданное место.
    pub async fn add_place(&self, p: &NewPlace) -> Result<Place> {
        let row = sqlx::query(
            "INSERT INTO places (name, description) VALUES ($1, $2) \
             RETURNING id, name, description",
        )
        .bind(&p.name)
        .bind(&p.description)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from_db)?;
        Ok(place_from_row(row))
    }

    /// Список всех мест.
    pub async fn list_places(&self) -> Result<Vec<Place>> {
        let rows = sqlx::query("SELECT id, name, description FROM places ORDER BY name, id")
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::from_db)?;
        Ok(rows.into_iter().map(place_from_row).collect())
    }

    /// Получить место по id.
    pub async fn get_place(&self, id: i64) -> Result<Place> {
        let row = sqlx::query("SELECT id, name, description FROM places WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(StoreError::from_db)?
            .ok_or(StoreError::NotFound { entity: "place", id })?;
        Ok(place_from_row(row))
    }

    /// Изменить место.
    pub async fn update_place(&self, id: i64, name: &str, description: Option<&str>) -> Result<Place> {
        let row = sqlx::query(
            "UPDATE places SET name = $2, description = $3 WHERE id = $1 \
             RETURNING id, name, description",
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from_db)?
        .ok_or(StoreError::NotFound { entity: "place", id })?;
        Ok(place_from_row(row))
    }

    /// Удалить место. Ошибка, если в нём ещё есть вещи (ON DELETE RESTRICT).
    pub async fn delete_place(&self, id: i64) -> Result<()> {
        let res = sqlx::query("DELETE FROM places WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(StoreError::from_db)?;
        if res.rows_affected() == 0 {
            return Err(StoreError::NotFound { entity: "place", id });
        }
        Ok(())
    }

    // ============================ ВЕЩИ ============================

    /// Добавить вещь (в указанное место) и сразу назначить ей атрибуты.
    ///
    /// Атрибуты задаются парами `attribute = "Цвет", value = "Красный"`;
    /// отсутствующие атрибуты и значения создаются автоматически.
    pub async fn add_item(&self, it: &NewItem) -> Result<Item> {
        let mut tx = self.pool.begin().await.map_err(StoreError::from_db)?;

        let row = sqlx::query(
            "INSERT INTO items (place_id, name, description, photo) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, place_id, name, description, photo",
        )
        .bind(it.place_id)
        .bind(&it.name)
        .bind(&it.description)
        .bind(it.photo.as_deref())
        .fetch_one(&mut *tx)
        .await
        .map_err(StoreError::from_db)?;
        let item = item_from_row(row);

        for pair in &it.attributes {
            upsert_attr_pair_tx(&mut tx, &pair.attribute, &pair.value, item.id)
                .await
                .map_err(StoreError::from_db)?;
        }

        tx.commit().await.map_err(StoreError::from_db)?;
        Ok(item)
    }

    /// Список всех вещей.
    pub async fn list_items(&self) -> Result<Vec<Item>> {
        let rows = sqlx::query(
            "SELECT id, place_id, name, description, photo FROM items ORDER BY name, id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from_db)?;
        Ok(rows.into_iter().map(item_from_row).collect())
    }

    /// Список вещей в конкретном месте.
    pub async fn list_items_in_place(&self, place_id: i64) -> Result<Vec<Item>> {
        let rows = sqlx::query(
            "SELECT id, place_id, name, description, photo FROM items \
             WHERE place_id = $1 ORDER BY name, id",
        )
        .bind(place_id)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from_db)?;
        Ok(rows.into_iter().map(item_from_row).collect())
    }

    /// Получить вещь по id.
    pub async fn get_item(&self, id: i64) -> Result<Item> {
        let row = sqlx::query(
            "SELECT id, place_id, name, description, photo FROM items WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from_db)?
        .ok_or(StoreError::NotFound { entity: "item", id })?;
        Ok(item_from_row(row))
    }

    /// Изменить вещь (место, имя, описание, фото — что передано, то обновляется).
    pub async fn update_item(
        &self,
        id: i64,
        place_id: Option<i64>,
        name: Option<&str>,
        description: Option<Option<&str>>,
        photo: Option<Option<&[u8]>>,
    ) -> Result<Item> {
        let row = sqlx::query(
            "UPDATE items SET \
               place_id    = COALESCE($2, place_id), \
               name        = COALESCE($3, name), \
               description = COALESCE($4, description), \
               photo       = COALESCE($5, photo) \
             WHERE id = $1 \
             RETURNING id, place_id, name, description, photo",
        )
        .bind(id)
        .bind(place_id)
        .bind(name)
        .bind(description.flatten())
        .bind(photo.flatten())
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from_db)?
        .ok_or(StoreError::NotFound { entity: "item", id })?;
        Ok(item_from_row(row))
    }

    /// Переместить вещь в другое место.
    pub async fn move_item(&self, item_id: i64, new_place_id: i64) -> Result<Item> {
        let row = sqlx::query(
            "UPDATE items SET place_id = $2 WHERE id = $1 \
             RETURNING id, place_id, name, description, photo",
        )
        .bind(item_id)
        .bind(new_place_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from_db)?
        .ok_or(StoreError::NotFound {
            entity: "item",
            id: item_id,
        })?;
        Ok(item_from_row(row))
    }

    /// Удалить вещь (назначения атрибутов удаляются каскадно).
    pub async fn delete_item(&self, id: i64) -> Result<()> {
        let res = sqlx::query("DELETE FROM items WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(StoreError::from_db)?;
        if res.rows_affected() == 0 {
            return Err(StoreError::NotFound { entity: "item", id });
        }
        Ok(())
    }

    // ======================= АТРИБУТЫ / ЗНАЧЕНИЯ =======================

    /// Добавить атрибут (уникальное имя).
    pub async fn add_attribute(&self, name: &str) -> Result<Attribute> {
        let row = sqlx::query("INSERT INTO attributes (name) VALUES ($1) RETURNING id, name")
            .bind(name)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from_db)?;
        Ok(Attribute {
            id: row.get("id"),
            name: row.get("name"),
        })
    }

    /// Список всех атрибутов.
    pub async fn list_attributes(&self) -> Result<Vec<Attribute>> {
        let rows = sqlx::query("SELECT id, name FROM attributes ORDER BY name, id")
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::from_db)?;
        Ok(rows
            .into_iter()
            .map(|r| Attribute {
                id: r.get("id"),
                name: r.get("name"),
            })
            .collect())
    }

    /// Удалить атрибут (его значения и назначения удаляются каскадно).
    pub async fn delete_attribute(&self, id: i64) -> Result<()> {
        let res = sqlx::query("DELETE FROM attributes WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(StoreError::from_db)?;
        if res.rows_affected() == 0 {
            return Err(StoreError::NotFound { entity: "attribute", id });
        }
        Ok(())
    }

    /// Добавить значение атрибута по id атрибута.
    pub async fn add_attribute_value(&self, attribute_id: i64, value: &str) -> Result<AttributeValue> {
        let mut tx = self.pool.begin().await.map_err(StoreError::from_db)?;
        let av = insert_attribute_value_tx(&mut tx, attribute_id, value)
            .await
            .map_err(StoreError::from_db)?;
        tx.commit().await.map_err(StoreError::from_db)?;
        Ok(av)
    }

    /// Добавить значение атрибута по имени атрибута (атрибут создаётся, если его нет).
    pub async fn add_attribute_value_by_name(
        &self,
        attribute_name: &str,
        value: &str,
    ) -> Result<AttributeValue> {
        let mut tx = self.pool.begin().await.map_err(StoreError::from_db)?;
        let attr_id = ensure_attribute_tx(&mut tx, attribute_name)
            .await
            .map_err(StoreError::from_db)?;
        let av = insert_attribute_value_tx(&mut tx, attr_id, value)
            .await
            .map_err(StoreError::from_db)?;
        tx.commit().await.map_err(StoreError::from_db)?;
        Ok(av)
    }

    /// Список всех значений атрибутов (с именем родительского атрибута).
    pub async fn list_attribute_values(&self) -> Result<Vec<AttributeValue>> {
        let rows = sqlx::query(
            "SELECT av.id, av.attribute_id, a.name AS attribute_name, av.value \
             FROM attribute_values av JOIN attributes a ON a.id = av.attribute_id \
             ORDER BY a.name, av.value, av.id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from_db)?;
        Ok(rows.into_iter().map(attr_value_from_row).collect())
    }

    /// Удалить значение атрибута (назначения вещам удаляются каскадно).
    pub async fn delete_attribute_value(&self, id: i64) -> Result<()> {
        let res = sqlx::query("DELETE FROM attribute_values WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(StoreError::from_db)?;
        if res.rows_affected() == 0 {
            return Err(StoreError::NotFound { entity: "attribute value", id });
        }
        Ok(())
    }

    // ==================== НАЗНАЧЕНИЕ АТРИБУТОВ ВЕЩАМ ====================

    /// Назначить вещи пару «атрибут = значение» (создавая недостающие записи).
    pub async fn assign_attribute_to_item(
        &self,
        item_id: i64,
        attribute: &str,
        value: &str,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(StoreError::from_db)?;
        upsert_attr_pair_tx(&mut tx, attribute, value, item_id)
            .await
            .map_err(StoreError::from_db)?;
        tx.commit().await.map_err(StoreError::from_db)?;
        Ok(())
    }

    /// Снять у вещи значение атрибута (по id значения).
    pub async fn unassign_attribute_from_item(&self, item_id: i64, attribute_value_id: i64) -> Result<()> {
        let res = sqlx::query(
            "DELETE FROM item_attribute_values WHERE item_id = $1 AND attribute_value_id = $2",
        )
        .bind(item_id)
        .bind(attribute_value_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from_db)?;
        if res.rows_affected() == 0 {
            return Err(StoreError::NotFound {
                entity: "item attribute assignment",
                id: attribute_value_id,
            });
        }
        Ok(())
    }

    // ======================== ИНФОРМАЦИЯ О ВЕЩИ ========================

    /// Полная информация о вещи: сама вещь, её местонахождение и список атрибутов.
    pub async fn item_info(&self, item_id: i64) -> Result<ItemInfo> {
        let item_row = sqlx::query(
            "SELECT id, place_id, name, description, photo FROM items WHERE id = $1",
        )
        .bind(item_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from_db)?
        .ok_or(StoreError::NotFound { entity: "item", id: item_id })?;
        let item = item_from_row(item_row);

        let place_row = sqlx::query("SELECT id, name, description FROM places WHERE id = $1")
            .bind(item.place_id)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from_db)?;
        let place = place_from_row(place_row);

        let attrs = self.item_attributes(item_id).await?;
        Ok(ItemInfo {
            item,
            place,
            attributes: attrs,
        })
    }

    /// Только список атрибутов (значений) вещи.
    pub async fn item_attributes(&self, item_id: i64) -> Result<Vec<AttributeValue>> {
        let rows = sqlx::query(
            "SELECT av.id, av.attribute_id, a.name AS attribute_name, av.value \
             FROM item_attribute_values iav \
             JOIN attribute_values av ON av.id = iav.attribute_value_id \
             JOIN attributes a ON a.id = av.attribute_id \
             WHERE iav.item_id = $1 \
             ORDER BY a.name, av.value",
        )
        .bind(item_id)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from_db)?;
        Ok(rows.into_iter().map(attr_value_from_row).collect())
    }

    /// Поиск вещей по значению атрибута.
    pub async fn find_items_by_attribute_value(
        &self,
        attribute: &str,
        value: &str,
    ) -> Result<Vec<Item>> {
        let rows = sqlx::query(
            "SELECT i.id, i.place_id, i.name, i.description, i.photo \
             FROM items i \
             JOIN item_attribute_values iav ON iav.item_id = i.id \
             JOIN attribute_values av ON av.id = iav.attribute_value_id \
             JOIN attributes a ON a.id = av.attribute_id \
             WHERE a.name = $1 AND av.value = $2 \
             ORDER BY i.name, i.id",
        )
        .bind(attribute)
        .bind(value)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from_db)?;
        Ok(rows.into_iter().map(item_from_row).collect())
    }
}

// ---------------------------------------------------------------------
// Вспомогательные функции внутри транзакций
// ---------------------------------------------------------------------

/// Разбить SQL-файл на отдельные инструкции (учитывая `--` комментарии).
fn split_sql_statements(sql: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let bytes = sql.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // пропуск строчных комментариев
        if bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b';' {
            let chunk = sql[start..i].trim();
            if !chunk.is_empty() {
                out.push(chunk);
            }
            start = i + 1;
            i += 1;
            continue;
        }
        i += 1;
    }
    let tail = sql[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn trim_for_msg(s: &str) -> String {
    let one_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.len() > 80 {
        format!("{}…", &one_line[..80])
    } else {
        one_line
    }
}

/// Найти id атрибута по имени, создать при отсутствии.
async fn ensure_attribute_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    name: &str,
) -> std::result::Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO attributes (name) VALUES ($1) \
         ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name \
         RETURNING id",
    )
    .bind(name)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.get("id"))
}

/// Вставить значение атрибута, вернуть полную запись.
async fn insert_attribute_value_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    attribute_id: i64,
    value: &str,
) -> std::result::Result<AttributeValue, sqlx::Error> {
    let row = sqlx::query(
        "WITH ins AS ( \
             INSERT INTO attribute_values (attribute_id, value) VALUES ($1, $2) \
             ON CONFLICT (attribute_id, value) DO NOTHING \
             RETURNING id, attribute_id, value \
           ), sel AS ( \
             SELECT id, attribute_id, value FROM ins \
             UNION ALL \
             SELECT av.id, av.attribute_id, av.value FROM attribute_values av \
             WHERE NOT EXISTS (SELECT 1 FROM ins) \
               AND av.attribute_id = $1 AND av.value = $2 \
           ) \
           SELECT sel.id, sel.attribute_id, a.name AS attribute_name, sel.value \
           FROM sel JOIN attributes a ON a.id = sel.attribute_id",
    )
    .bind(attribute_id)
    .bind(value)
    .fetch_one(&mut **tx)
    .await?;
    Ok(attr_value_from_row(row))
}

/// Обеспечить существование пары «атрибут=значение» и назначить её вещи.
async fn upsert_attr_pair_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    attribute: &str,
    value: &str,
    item_id: i64,
) -> std::result::Result<(), sqlx::Error> {
    let attr_id = ensure_attribute_tx(tx, attribute).await?;
    let av = insert_attribute_value_tx(tx, attr_id, value).await?;
    sqlx::query(
        "INSERT INTO item_attribute_values (item_id, attribute_value_id) \
         VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(item_id)
    .bind(av.id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

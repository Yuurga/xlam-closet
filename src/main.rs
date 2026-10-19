//! CLI-обёртка над библиотекой closet.

use std::io::Read;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use closet::{ItemAttrPair, NewItem, NewPlace, Store};

#[derive(Parser)]
#[command(name = "closet", version, about = "Хранилище вещей и их местоположения (PostgreSQL)")]
struct Cli {
    /// Строка подключения к PostgreSQL.
    /// По умолчанию берётся из переменной окружения DATABASE_URL.
    #[arg(short, long, env = "DATABASE_URL", global = true)]
    database_url: Option<String>,

    #[command(subcommand)]
    command: Cmd,
}

impl Cli {
    fn db_url(&self) -> Result<&str> {
        self.database_url
            .as_deref()
            .context("не задана строка подключения: используйте --database-url или переменную окружения DATABASE_URL")
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Создать таблицы схемы, если их ещё нет
    Init,

    /// Добавить место
    AddPlace {
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Список мест
    ListPlaces,

    /// Добавить вещь (атрибуты в формате --attr "Цвет=Красный")
    AddItem {
        /// id места, куда кладётся вещь
        place_id: i64,
        name: String,
        #[arg(long)]
        description: Option<String>,
        /// путь к файлу с фото (или "-" — прочитать stdin)
        #[arg(long)]
        photo: Option<String>,
        /// пара атрибут=значение, можно указывать многократно
        #[arg(long = "attr")]
        attrs: Vec<String>,
    },
    /// Список вещей
    ListItems,
    /// Список вещей в месте
    ItemsInPlace { place_id: i64 },
    /// Полная информация о вещи: местонахождение + атрибуты
    ItemInfo { item_id: i64 },
    /// Переместить вещь в другое место
    MoveItem { item_id: i64, to_place_id: i64 },
    /// Удалить вещь
    DeleteItem { item_id: i64 },
    /// Назначить вещи атрибут (--attr Цвет=Красный)
    AssignAttr {
        item_id: i64,
        #[arg(long)]
        attr: String,
    },

    /// Добавить атрибут
    AddAttribute { name: String },
    /// Список атрибутов
    ListAttributes,
    /// Добавить значение атрибута (по id или имени атрибута)
    AddAttrValue {
        #[arg(long)]
        attribute_id: Option<i64>,
        #[arg(long)]
        attribute: Option<String>,
        value: String,
    },
    /// Список всех значений атрибутов
    ListAttrValues,

    /// Найти вещи по значению атрибута (пример: find Color Red)
    Find { attribute: String, value: String },
}

fn parse_attr_pair(s: &str) -> Result<ItemAttrPair> {
    let (a, v) = s
        .split_once('=')
        .with_context(|| format!("ожидался формат \"Атрибут=Значение\", получено \"{s}\""))?;
    let a = a.trim();
    let v = v.trim();
    anyhow::ensure!(
        !a.is_empty() && !v.is_empty(),
        "пустое имя атрибута или значение в \"{s}\""
    );
    Ok(ItemAttrPair {
        attribute: a.to_string(),
        value: v.to_string(),
    })
}

fn read_photo(path: &str) -> Result<Option<Vec<u8>>> {
    if path == "-" {
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .context("чтение фото из stdin")?;
        return Ok((!buf.is_empty()).then_some(buf));
    }
    let data = std::fs::read(path).with_context(|| format!("чтение файла фото {path}"))?;
    Ok(Some(data))
}

/// Сериализация в JSON. Для поля `photo` печатается только размер,
/// чтобы не вываливать бинарные данные в консоль.
fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    let mut v = serde_json::to_value(value)?;
    match &mut v {
        serde_json::Value::Array(items) => {
            for it in items {
                hide_photo(it);
            }
        }
        other => hide_photo(other),
    }
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}

fn hide_photo(v: &mut serde_json::Value) {
    // скрываем фото и у самой вещи, и у вложенного объекта item (ItemInfo)
    if let Some(obj) = v.as_object_mut() {
        if let Some(item) = obj.get_mut("item") {
            hide_photo(item);
        }
        if let Some(ph) = obj.get_mut("photo") {
            if !ph.is_null() {
                let n = ph.as_array().map(|a| a.len()).unwrap_or(0);
                *ph = serde_json::json!(format!("<{n} bytes>"));
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // переменные окружения из .env (если есть)
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();
    let store = Store::connect(cli.db_url()?, 5).await?;

    match cli.command {
        Cmd::Init => {
            store.migrate().await?;
            println!("ok");
        }
        Cmd::AddPlace { name, description } => {
            let p = store.add_place(&NewPlace { name, description }).await?;
            print_json(&p)?;
        }
        Cmd::ListPlaces => {
            print_json(&store.list_places().await?)?;
        }
        Cmd::AddItem {
            place_id,
            name,
            description,
            photo,
            attrs,
        } => {
            let attributes = attrs
                .iter()
                .map(|s| parse_attr_pair(s))
                .collect::<Result<Vec<_>>>()?;
            let photo_data = match photo.as_deref() {
                Some(p) => read_photo(p)?,
                None => None,
            };
            let it = store
                .add_item(&NewItem {
                    place_id,
                    name,
                    description,
                    photo: photo_data,
                    attributes,
                })
                .await?;
            print_json(&it)?;
        }
        Cmd::ListItems => {
            print_json(&store.list_items().await?)?;
        }
        Cmd::ItemsInPlace { place_id } => {
            print_json(&store.list_items_in_place(place_id).await?)?;
        }
        Cmd::ItemInfo { item_id } => {
            print_json(&store.item_info(item_id).await?)?;
        }
        Cmd::MoveItem {
            item_id,
            to_place_id,
        } => {
            print_json(&store.move_item(item_id, to_place_id).await?)?;
        }
        Cmd::DeleteItem { item_id } => {
            store.delete_item(item_id).await?;
            println!("ok");
        }
        Cmd::AssignAttr { item_id, attr } => {
            let pair = parse_attr_pair(&attr)?;
            store
                .assign_attribute_to_item(item_id, &pair.attribute, &pair.value)
                .await?;
            println!("ok");
        }
        Cmd::AddAttribute { name } => {
            print_json(&store.add_attribute(&name).await?)?;
        }
        Cmd::ListAttributes => {
            print_json(&store.list_attributes().await?)?;
        }
        Cmd::AddAttrValue {
            attribute_id,
            attribute,
            value,
        } => {
            let av = match (attribute_id, attribute) {
                (Some(id), _) => store.add_attribute_value(id, &value).await?,
                (None, Some(name)) => store.add_attribute_value_by_name(&name, &value).await?,
                (None, None) => {
                    anyhow::bail!("нужно указать --attribute-id <id> или --attribute <имя>")
                }
            };
            print_json(&av)?;
        }
        Cmd::ListAttrValues => {
            print_json(&store.list_attribute_values().await?)?;
        }
        Cmd::Find { attribute, value } => {
            print_json(&store.find_items_by_attribute_value(&attribute, &value).await?)?;
        }
    }
    Ok(())
}

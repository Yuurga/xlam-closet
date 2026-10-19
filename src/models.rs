//! Доменные модели приложения.

use serde::{Deserialize, Serialize};

/// Место хранения (дом, гараж, шкаф и т.п.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Place {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
}

/// Вещь, привязанная к месту.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub place_id: i64,
    pub name: String,
    pub description: Option<String>,
    /// Фото вещи (двоичные данные, колонка `photo BLOB`/`BYTEA`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub photo: Option<Vec<u8>>,
}

/// Атрибут (например, «Цвет», «Размер»).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute {
    pub id: i64,
    pub name: String,
}

/// Значение атрибута (например, «Красный» для атрибута «Цвет»).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeValue {
    pub id: i64,
    pub attribute_id: i64,
    /// Имя родительского атрибута (для удобства отображения).
    pub attribute_name: String,
    pub value: String,
}

/// Пары «атрибут-значение», назначенные вещи (для добавления/обновления).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemAttrPair {
    pub attribute: String,
    pub value: String,
}

/// Полная информация о вещи: где лежит и какие атрибуты назначены.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemInfo {
    pub item: Item,
    pub place: Place,
    pub attributes: Vec<AttributeValue>,
}

/// Параметры создания нового места.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewPlace {
    pub name: String,
    pub description: Option<String>,
}

/// Параметры создания новой вещи.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewItem {
    pub place_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub photo: Option<Vec<u8>>,
    /// Атрибуты, которые сразу назначаются вещи.
    pub attributes: Vec<ItemAttrPair>,
}

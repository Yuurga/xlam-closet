//! # closet
//!
//! Библиотека для хранения информации о вещах и их местоположении
//! (PostgreSQL, sqlx).
//!
//! Пример:
//! ```no_run
//! use closet::{Store, NewPlace, NewItem, ItemAttrPair};
//!
//! # async fn demo() -> anyhow::Result<()> {
//! let store = Store::connect("postgres://user:pass@localhost/closet", 5).await?;
//! store.migrate().await?;
//!
//! let place = store.add_place(&NewPlace { name: "Гараж".into(), description: None }).await?;
//! let item = store.add_item(&NewItem {
//!     place_id: place.id,
//!     name: "Дрель".into(),
//!     description: None,
//!     photo: None,
//!     attributes: vec![ItemAttrPair { attribute: "Цвет".into(), value: "Зелёный".into() }],
//! }).await?;
//!
//! let info = store.item_info(item.id).await?; // вещь + место + атрибуты
//! println!("{} лежит в «{}»", info.item.name, info.place.name);
//! # Ok(())
//! # }

pub mod error;
pub mod models;
pub mod store;

pub use error::{Result, StoreError};
pub use models::{
    Attribute, AttributeValue, Item, ItemAttrPair, ItemInfo, NewItem, NewPlace, Place,
};
pub use store::Store;

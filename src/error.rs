//! Ошибки приложения.

use thiserror::Error;

/// Универсальный тип ошибки для всех операций хранилища.
#[derive(Debug, Error)]
pub enum StoreError {
    /// Ошибка взаимодействия с PostgreSQL.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// Запрошенная сущность (место/вещь/атрибут) не найдена.
    #[error("{entity} with id {id} not found")]
    NotFound { entity: &'static str, id: i64 },

    /// Нарушение уникальности (например, дубликат имени атрибута).
    #[error("duplicate value for unique constraint: {0}")]
    Duplicate(String),

    /// Ограничение ON DELETE RESTRICT: в месте ещё есть вещи.
    #[error("operation violates a referential constraint: {0}")]
    ConstraintViolation(String),

    /// Ошибка ввода-вывода (например, чтение файла фото).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

impl StoreError {
    /// Маппинг ошибок драйвера базы данных в доменные ошибки.
    pub(crate) fn from_db(err: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db_err) = &err {
            match db_err.code().as_deref() {
                // unique_violation
                Some("23505") => {
                    return StoreError::Duplicate(db_err.to_string());
                }
                // foreign_key_violation / restricted_violation
                Some("23503") | Some("23001") => {
                    return StoreError::ConstraintViolation(db_err.to_string());
                }
                _ => {}
            }
        }
        StoreError::Database(err)
    }
}

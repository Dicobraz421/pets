//! Ошибки обновления.

use std::io;

/// Почему подпись файла не принята.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    /// Файл подписи не разобран или подписан устаревшим способом (без хэша).
    #[error("signature file is malformed")]
    Malformed,
    /// Подпись выдана другому файлу: в подписанном комментарии нет `file:<имя артефакта>`.
    #[error("signature belongs to another file")]
    WrongFile,
    /// Подпись сделана ключом, которого нет среди вшитых.
    #[error("signature is made with an unknown key")]
    UnknownKey,
    /// Содержимое файла не соответствует подписи.
    #[error("file does not match its signature")]
    Mismatch,
}

/// Ошибка проверки, скачивания или установки обновления.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum UpdateError {
    /// Обновление выключено групповой политикой: сетевых запросов нет.
    #[error("updates are disabled by group policy")]
    DisabledByPolicy,
    /// Сеть: соединение, TLS, таймаут.
    #[error("network: {0}")]
    Network(String),
    /// Сервер ответил ошибкой HTTP.
    #[error("server answered HTTP {0}")]
    Http(u16),
    /// Ответ GitHub не разобран.
    #[error("release list is not valid: {0}")]
    Parse(#[from] serde_json::Error),
    /// Отменено пользователем.
    #[error("cancelled")]
    Cancelled,
    /// Файловая операция `what`.
    #[error("{what}: {source}")]
    Io {
        /// Что делали.
        what: &'static str,
        /// Ошибка ОС.
        #[source]
        source: io::Error,
    },
    /// Скачано не столько, сколько объявлено в выпуске.
    #[error("downloaded {got} bytes, release declares {expected}")]
    SizeMismatch {
        /// Размер из выпуска.
        expected: u64,
        /// Получено.
        got: u64,
    },
    /// Подпись не принята: файл удалён и не запускается.
    #[error("signature rejected: {0}")]
    Signature(#[from] SignatureError),
    /// Вшитый открытый ключ не разобран (ошибка сборки продукта).
    #[error("embedded public key is not valid")]
    BadPublicKey,
    /// Архив portable не распакован.
    #[error("archive: {0}")]
    Archive(String),
    /// `msiexec` завершился с ошибкой.
    #[error("installer failed with code {0}")]
    InstallerFailed(i32),
    /// Неверные входные данные.
    #[error("invalid input: {0}")]
    Invalid(&'static str),
}

/// Обёртка файловой ошибки с названием операции.
pub(crate) fn io_err(what: &'static str) -> impl FnOnce(io::Error) -> UpdateError {
    move |source| UpdateError::Io { what, source }
}

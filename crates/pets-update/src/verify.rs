//! Проверка подписи minisign (ed25519 по хэшу BLAKE2b) вшитым открытым ключом.
//!
//! Подпись привязана не только к содержимому, но и к имени файла: в подписанном комментарии должен быть
//! `file:<имя артефакта>` (minisign пишет его сам). Имя содержит продукт и версию, поэтому старый подписанный
//! установщик или файл другого продукта под видом нового выпуска не проходит.

#[cfg(not(windows))]
use std::fs;
#[cfg(windows)]
use std::fs::File;
use std::fs::OpenOptions;
#[cfg(not(windows))]
use std::io::Cursor;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use minisign_verify::{PublicKey, Signature};

use crate::error::io_err;
use crate::{SignatureError, UpdateError};

/// Проверенное содержимое, из которого ставится обновление. Windows — сам файл, закрытый от записи и удаления
/// (`FILE_SHARE_READ`), пока значение живо: `msiexec` ставит по пути. Unix такой блокировки не даёт (тот же
/// пользователь перепишет файл через другой дескриптор), поэтому файл читается один раз в память процесса: подпись
/// проверяется и архив распаковывается из одной и той же копии.
#[derive(Debug)]
pub(crate) struct Payload {
    #[cfg(windows)]
    inner: File,
    #[cfg(not(windows))]
    inner: Cursor<Vec<u8>>,
}

impl Read for Payload {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Seek for Payload {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// Больше скачанный артефакт быть не может: копия в памяти — не дальше этого предела.
#[cfg(not(windows))]
const MAX_PAYLOAD: u64 = 1 << 30;

/// Открыть файл на чтение, запретив другим процессам запись и удаление, пока дескриптор жив.
#[cfg(windows)]
pub(crate) fn open_locked(path: &Path) -> std::io::Result<Payload> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    let inner = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(path)?;
    Ok(Payload { inner })
}

/// Прочитать файл в память одним проходом: дальше подмена файла на диске не влияет ни на проверку, ни на установку.
/// Ссылка вместо файла — ошибка.
#[cfg(not(windows))]
pub(crate) fn open_locked(path: &Path) -> std::io::Result<Payload> {
    use std::os::unix::fs::MetadataExt;
    let link = fs::symlink_metadata(path)?;
    if link.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "downloaded file is a link",
        ));
    }
    let file = OpenOptions::new().read(true).open(path)?;
    // Между проверкой и открытием путь могли заменить ссылкой: открытый файл — тот же обычный файл (том и inode).
    let opened = file.metadata()?;
    if !opened.is_file() || opened.dev() != link.dev() || opened.ino() != link.ino() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "downloaded file changed while opening",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PAYLOAD + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PAYLOAD {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "downloaded file is too large",
        ));
    }
    Ok(Payload {
        inner: Cursor::new(bytes),
    })
}

/// Проверить файл `path` с именем артефакта `name` по тексту подписи. Возвращает проверенное содержимое
/// ([`Payload`]): ставить можно только из него, не из пути.
pub(crate) fn verify_file(keys: &[&str], name: &str, path: &Path, signature: &str) -> Result<Payload, UpdateError> {
    let signature = Signature::decode(signature).map_err(|_| SignatureError::Malformed)?;
    let for_this_file = signature
        .trusted_comment()
        .split_whitespace()
        .any(|token| token.strip_prefix("file:") == Some(name));
    if !for_this_file {
        return Err(SignatureError::WrongFile.into());
    }
    let mut file = open_locked(path).map_err(io_err("open downloaded file"))?;
    for key in keys {
        let key = PublicKey::from_base64(key).map_err(|_| UpdateError::BadPublicKey)?;
        let mut verifier = match key.verify_stream(&signature) {
            Ok(v) => v,
            Err(minisign_verify::Error::UnexpectedKeyId) => continue,
            Err(_) => return Err(SignatureError::Malformed.into()),
        };
        file.seek(SeekFrom::Start(0)).map_err(io_err("read downloaded file"))?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf).map_err(io_err("read downloaded file"))?;
            if n == 0 {
                break;
            }
            verifier.update(&buf[..n]);
        }
        return match verifier.finalize() {
            Ok(()) => {
                file.seek(SeekFrom::Start(0)).map_err(io_err("read downloaded file"))?;
                Ok(file)
            }
            Err(_) => Err(SignatureError::Mismatch.into()),
        };
    }
    Err(SignatureError::UnknownKey.into())
}

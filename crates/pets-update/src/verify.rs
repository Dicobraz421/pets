//! Проверка подписи minisign (ed25519 по хэшу BLAKE2b) вшитым открытым ключом.
//!
//! Подпись привязана не только к содержимому, но и к имени файла: в подписанном комментарии должен быть
//! `file:<имя артефакта>` (minisign пишет его сам). Имя содержит продукт и версию, поэтому старый подписанный
//! установщик или файл другого продукта под видом нового выпуска не проходит.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use minisign_verify::{PublicKey, Signature};

use crate::error::io_err;
use crate::{SignatureError, UpdateError};

/// Открыть файл на чтение, запретив другим процессам запись и удаление, пока дескриптор жив.
pub(crate) fn open_locked(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_SHARE_READ: u32 = 1;
        options.share_mode(FILE_SHARE_READ);
    }
    options.open(path)
}

/// Проверить файл `path` с именем артефакта `name` по тексту подписи. Возвращает дескриптор, который держит
/// файл закрытым от записи: пока он жив, проверенное содержимое не подменить.
pub(crate) fn verify_file(keys: &[&str], name: &str, path: &Path, signature: &str) -> Result<File, UpdateError> {
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
            Ok(()) => Ok(file),
            Err(_) => Err(SignatureError::Mismatch.into()),
        };
    }
    Err(SignatureError::UnknownKey.into())
}

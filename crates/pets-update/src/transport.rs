//! Сеть. Весь HTTP крейта идёт через [`Transport`]: в продукте — [`UreqTransport`], в тестах — подмена.

use std::io::Read;
use std::time::Duration;

use crate::UpdateError;

/// Ответ сервера: длина (если объявлена) и тело.
pub struct Response {
    /// `Content-Length`.
    pub len: Option<u64>,
    /// Тело.
    pub body: Box<dyn Read + Send>,
}

/// GET-запрос по HTTPS. Вызывается только из фоновых потоков продукта.
pub trait Transport: Send + Sync {
    /// Выполнить GET; перенаправления — внутри.
    fn get(&self, url: &str) -> Result<Response, UpdateError>;
}

/// Заголовок `User-Agent`: одинаков у всех, данных пользователя и машины нет.
pub const USER_AGENT: &str = "pets-update";

/// Настоящая сеть: `ureq` + TLS Windows (SChannel), системный прокси, только HTTPS, таймауты.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    /// Соединение — 15 с, ответ — 30 с, простой при чтении тела — 30 с.
    pub fn new() -> UreqTransport {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(5)
            .user_agent(USER_AGENT)
            .tls_config(tls)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .build()
            .new_agent();
        UreqTransport { agent }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        UreqTransport::new()
    }
}

impl Transport for UreqTransport {
    fn get(&self, url: &str) -> Result<Response, UpdateError> {
        let mut request = self.agent.get(url);
        if url.starts_with("https://api.github.com/") {
            request = request
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
        }
        let response = request.call().map_err(|e| match e {
            ureq::Error::StatusCode(code) => UpdateError::Http(code),
            other => UpdateError::Network(other.to_string()),
        })?;
        let len = response.body().content_length();
        Ok(Response {
            len,
            body: Box::new(response.into_body().into_reader()),
        })
    }
}

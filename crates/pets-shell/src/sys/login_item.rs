//! Запуск при входе в систему на macOS 13+: `SMAppService.mainApp` — само приложение в «Объектах входа» Системных
//! настроек, без вспомогательного агента и plist. Нужен `.app` (вне пакета — ошибка ОС). Пользователь может выключить
//! объект входа в настройках: тогда [`State::RequiresApproval`], и включить его там же ([`open_settings`]).

use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::ShellError;

/// Состояние объекта входа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Не зарегистрирован.
    Off,
    /// Запускается при входе.
    On,
    /// Зарегистрирован, но выключен пользователем или ждёт его разрешения в «Объектах входа».
    RequiresApproval,
}

fn service() -> objc2::rc::Retained<SMAppService> {
    // SAFETY: фабрика без аргументов; объект живёт, пока его держит `Retained`.
    unsafe { SMAppService::mainAppService() }
}

/// Текущее состояние. Любой поток.
pub fn state() -> State {
    // SAFETY: чтение свойства живого объекта.
    match unsafe { service().status() } {
        SMAppServiceStatus::Enabled => State::On,
        SMAppServiceStatus::RequiresApproval => State::RequiresApproval,
        _ => State::Off,
    }
}

/// Включить (`register`) или выключить (`unregister`) запуск при входе.
pub fn set(on: bool) -> Result<(), ShellError> {
    // Не зарегистрирован — выключать нечего (`unregister` ответил бы ошибкой).
    if !on && state() == State::Off {
        return Ok(());
    }
    let service = service();
    // SAFETY: вызовы живого объекта; ошибка — NSError, переводится в текст.
    let result = unsafe {
        if on {
            service.registerAndReturnError()
        } else {
            service.unregisterAndReturnError()
        }
    };
    result.map_err(|e| ShellError::Os {
        what: if on {
            "SMAppService register"
        } else {
            "SMAppService unregister"
        },
        source: std::io::Error::other(e.localizedDescription().to_string()),
    })
}

/// «Системные настройки → Основные → Объекты входа».
pub fn open_settings() {
    // SAFETY: функция класса без аргументов.
    unsafe { SMAppService::openSystemSettingsLoginItems() }
}

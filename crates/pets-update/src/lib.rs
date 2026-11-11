//! Обновление приложений Pets (решение владельца 2026-11-11): один механизм для Урфина и Postman, без UI.
//!
//! Порядок работы продукта — в `README.md` крейта. Коротко:
//!
//! 1. первой строкой `main` — [`run_helper`]; при старте — [`take_result`];
//! 2. в фоне раз в час и по кнопке — [`Updater::check`] (сам решает, пора ли: раз в сутки);
//! 3. по согласию пользователя — [`Updater::download`] → [`Updater::verify`] → [`Updater::install`] → выход.
//!
//! Гарантии:
//!
//! - сеть — только здесь ([`Transport`]), только HTTPS, с таймаутами; в запросах нет данных пользователя;
//! - политика `AllowUpdate = 0` ([`Policy`]) — запросов нет вовсе; выключенная настройка — нет фоновых запросов;
//! - версия не новее текущей не предлагается; файл без верной подписи вшитым ключом удаляется и не запускается;
//! - `unsafe` — только в `sys` (чтение реестра).

mod error;
mod install;
mod policy;
mod product;
mod release;
mod sys;
mod transport;
mod updater;
mod verify;

pub use semver::Version;

pub use error::{SignatureError, UpdateError};
pub use install::{HELPER_FLAG, InstallPlan, UpdateResult, run_helper, take_result};
pub use policy::{ALLOW_UPDATE, POLICY_KEY, Policy};
pub use product::{ARCH, Channel, InstallKind, OS, Product, Settings};
pub use release::{SIGNATURE_EXT, Update, releases_url, select};
pub use transport::{Response, Transport, USER_AGENT, UreqTransport};
pub use updater::{
    CHECK_PERIOD, Cancel, CheckOutcome, CheckReason, DisabledBy, Downloaded, Progress, RETRY_PERIOD, Updater, Verified,
};

//! Доменные UI-хелперы shell/mcp карточек: заголовки, типы и raw-input.
//! Модуль оставлен фасадом; эвристики и форматирование вынесены в подпакеты.

#[path = "kind.rs"]
pub(in crate::thread) mod kind;
#[path = "raw.rs"]
pub(in crate::thread) mod raw;
#[path = "title.rs"]
pub(in crate::thread) mod title;

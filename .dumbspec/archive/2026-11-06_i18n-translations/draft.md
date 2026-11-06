# Запрос (сырой ввод, дословно)

/dumbspec 

- [ ] настроить используя crate rust-i18n, используем `_version: 2` и держим переводы в одном файле (см https://github.com/longbridge/rust-i18n#all-localized-texts-in-one-file )
- [ ] в настройках General появляется дропдаун с выбором языка, по умолчанию en
- [ ] добавляем перевод на
   * [ ] en (переносим ключи
   * [ ] ru (в первую очередь)
   * [ ] zh-CN
   * [ ] es
   * [ ] jp
   * [ ] ko

## Уточнения пользователя (шаг 5, before research)

- Применение языка: сразу в рантайме — через `rust_i18n::set_locale()`
  (https://raw.githubusercontent.com/longbridge/rust-i18n/refs/heads/main/README.md :
  «You can use `rust_i18n::set_locale()` to set the global locale at runtime, so
  that you don't have to specify the locale on each `t!` invocation»,
  `rust_i18n::locale()` читает текущий). Перезапуск не требуется.
- Охват строк: весь UI настроек + toast/overlay.
- Японский: код локали `ja` (в UI — «日本語»).
- Тексты: автоперевод (ru в первую очередь), затем правки пользователя.

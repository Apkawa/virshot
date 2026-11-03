# Бэклог задач

## core

- [ ] config.rs - общий код для чтения конфигов. сейчас часть конфига есть в logging

## toast

- [ ] выделим код связанный с отрисовкой в оверлее в `crate/toast` (переименуем) и добавим простейшее тестовое приложение для тестирования тостов

## Tests

- [x] все тесты внутри файлов вынести в папку tests/unit/ → сделано иначе: sibling
  `src/<module>.tests.rs` (нет `src/lib.rs`, visibility не расширялась), 232 теста
- [x] изучить возможность e2e приложения посредством xvfb. \\
    можно своровать идеи из .tmp/qa-harness/ и адаптировать под наши реалии. только linux
    → как хелперы ручного проверки: `scripts/ui-verify.sh` (окно/root-захват, hotkey-захват,
    трей через D-Bus, опция `--wm`); e2e-тесты не делались (решение пользователя)
- [x] профилирование памяти https://github.com/flamegraph-rs/flamegraph, пока только
    настроим рабочий процесс → `scripts/prof.sh` (`heap|leak|cpu|mem-count`); flamegraph
    не ставили (нет SVG-инструмента), анализ через `perf report --stdio`

## CI

- [ ] cargo-audit в CI (security advisories) — опционально.

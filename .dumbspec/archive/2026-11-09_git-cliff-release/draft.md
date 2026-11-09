# Draft — сырой запрос (2026-11-09)

/dumbspec
добавить dev deps git-cliff

Судя по документации, .tmp/git-cliff/website/docs/usage/bump-version.md он умеет в bump version
Я склонировал репо .tmp/git-cliff/website/docs/

в общем что я хочу при помощи этой тулзы. мб даже небольшой скрипт-хелпер  который вызывает гит клифф. типа scripts/release.sh

1) генерировать CHANGELOG.md (release.sh changelog
2) бампать версию (release.sh path|minor|major) (мы будем использовать ZeroVer семантику, т.е. 0.minor.patch, а major никогда не случится (ну или пока я не решу что все готово наконец)), оно так же создает git tag типа v.0.1.2

ci пока не входит в scope

## Уточнения (по ответам пользователя, 2026-11-09)

- Идём без further refinement.
- Tag: `v0.1.2` (без точки после `v`).
- Bump: правит версию, коммитит, ставит tag (tag на коммите с версией).
- `release.sh major` — запрещён (ошибка), включим позже когда "готово".
- git-cliff: `cargo install`, целевая версия зафиксирована в проекте.

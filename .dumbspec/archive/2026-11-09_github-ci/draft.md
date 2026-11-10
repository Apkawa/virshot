# Исходный запрос

Получен: 2026-11-09T18:19:00+03:00

/dumbspec
добавляем github ci
1) тестирование
2) сборка релиза

- [ ] Для релиза собираем под x86_64-pc-windows-msvc, aarch64-pc-windows-msvc, x86_64-unknown-linux-gnu
файл в формате `virshot-v<version>-<platform>-<aarch>.(zip|tar.gz)`
- [ ] генерируем sha512 в release.yaml (который лежит в релизе гитхаба), пример:
```yaml
version: 0.0.1
releaseDate: '2026-05-28T05:57:20.292Z'
commit_sha: 66440a1
files:
  - url: virshot-v0.0.1-win-arm64.zip
    type: aarch64-pc-windows-msvc
    sha512: yipxRSOrg0J13OCF0+JmVBqsihu1K4lFf2B6RooCW7dfPPyID7jHkhBFwVMSBigDb5wcKfdQOyD4lDthJUGUIA==
    size: 118912168
  - url: virshot-v0.0.1-win-x64.zip
    type: x86_64-pc-windows-msvc
    sha512: rvQetM+EGBEewql0ofvQkKOnyEtLHoQXyMEv+FK3AJQzmGJR18PcimHh//BRawDy8rxAn5FfzMamgvrMRbwFyQ==
    size: 126390832
  - url: virshot-v0.0.1-linux-x64.tar.gz
    type: x86_64-unknown-linux-gnu
    sha512: rvQetM+EGBEewql0ofvQkKOnyEtLHoQXyMEv+FK3AJQzmGJR18PcimHh//BRawDy8rxAn5FfzMamgvrMRbwFyQ==
    size: 126390832
```
- [ ] так же держим релиз с типом prerelease  в который собирается любой коммит из master
- [ ] в релиз пишем changelog который накопился между релизами (может можно использовать git cliff  и писать список изменений в stdout и использовать в релизе

## Уточнения (2026-11-09T18:24:28+03:00, отвечено пользователем)

- Имена артефактов — короткие токены из примера:
  `virshot-v<version>-win-x64.zip`, `virshot-v<version>-win-arm64.zip`,
  `virshot-v<version>-linux-x64.tar.gz`.
- Prerelease: **один rolling prerelease** для `master`, обновляется in-place
  (артефакты перезаписываются, старые ассеты удаляются).
- В архив — **только бинарь** (`virshot.exe` / `virshot`).
- Остальные открытые вопросы (содержимое CI-прогона, способ сборки
  `aarch64-pc-windows-msvc`, схема версии/тегов, триггер релиза) решаются в
  research и спецификации.

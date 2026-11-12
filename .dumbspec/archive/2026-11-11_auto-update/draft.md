# Автоматическое обновление

**Настройки в General**
- Дропдаун: проверять обновления — никогда / 1h / 1d / 1w
- Дропдаун: stable, nightly
- Чекбокс: автоматически устанавливать

**Логика при запуске (в фоне)**
1. Получаем список [релизов](https://github.com/Apkawa/virshot/releases/) по апи гитхаба, сравниваем `commit_sha` и версию с текущей. (для stable проверяем релизы, для nightly проверяем https://github.com/Apkawa/virshot/releases/tag/nightly pre-release
2. При несовпадении:
   - Если автоустановка выключена: показываем кнопку «Доступно обновление» → окно с описанием изменений и кнопками «Обновление» / «Отмена».
   - Если включена: переходим к шагу 3.

**Процесс установки**
Используем `self_replace` crate

3. Скачиваем архив во временную папку (`%TEMP%`), проверяем контрольную сумму используя release.yaml ассет релиза в котором есть ссылки, хеши и тд. Пример содержимого:
```bash
$ curl -L https://github.com/Apkawa/virshot/releases/download/nightly/release.yaml
version: 0.1.0
releaseDate: '2026-11-10T11:29:09.928Z'
commit_sha: f710773
files:
  - url: https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-win-x64.zip
    type: x86_64-pc-windows-msvc
    sha512: 2Gh+zeeFehW+ZXL5A30Q3pjMlbu7bSX8UjFkQgPZ5/fPUaQi+30grx2xiYdvTIS1CrH6afAV6hkKT3AU2a+feg==
    size: 3244195
  - url: https://github.com/Apkawa/virshot/releases/download/nightly/virshot-0.1.0-nightly-f710773-linux-x64.tar.gz
    type: x86_64-unknown-linux-gnu
    sha512: XnvJ12rWJOuXACymadNltd5jqvPBhxrE0zJ+6FmDPEGwmDmIpce6DdvZcXGzIr+E+4po2RLpyRGvb4FdHOOg/A==
    size: 5617967
```
4. Распаковываем новый бинарник во временную папку.
5. Вызываем **`self_replace::self_replace("/path/to/new/virshot.exe")`** — крейт сам переименует текущий запущенный `.exe`, подменит его новым и запланирует удаление временных файлов.
6. После успешной замены: перезапускаем приложение (или завершаемся, чтобы новый бинарник запустился при следующем старте).
7. При каждом старте приложение удаляет временные файлы обновления (`*.old`, архивы), которые могли остаться от предыдуских попыток.

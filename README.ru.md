
[English](./README.md) | Russian | [中文](./README.zh.md)

# ViRShot - Vibe Rust Shot

![ViRShot icon](./crates/icons/assets/png/app-128.png)

ViRShot - кроссплатформенная утилита для скриншотов написанная на Rust

Цель - легковесная утилита которая поддерживает кнопки геймпада без [Xbox Button Remapper](https://github.com/Adam777Z/xbox-controller-button-remapper). Этой функции вроде не было в других похожих проектах

## Функции

- Назначение кнопок геймпада
- Использование шаблонов для имени файла, в том числе и название окна
- Cохранение в png, jpeg, webp
- При скриншоте отображается сообщение на всех мониторах
- Желтая рамка вокруг окна или экрана во время скриншота - это особенность работы **Windows Graphics Capture (WGC)** 
- Перевод на English, Русский, Китайский упрощенный, Испанский, Японский, Корейский.
- Автообновление (отключаемое)
- Добавление в автозапуск (отключаемое)
- Windows, Linux (не тестировался)


## Скачивание

Скачайте со [страницы релизов](https://github.com/Apkawa/virshot/releases/)

- [nightly](https://github.com/Apkawa/virshot/releases/tag/nightly) - каждый коммит в master попадает туда
- [stable](https://github.com/Apkawa/virshot/releases/latest) - стабильный релиз
 
## Установка и запуск

Программа распространяется в portable режиме. распаковать в нужную папку и запустить, конфиг будет находиться рядом c exe.

Сертификата нет, поэтому будет сообщение от Defender SmartScreen вроде "Windows protected your PC". Выбираете "More info"  и запустить в любом случае.

## Назначение клавиш геймпада

Для начала необходимо отключить **Xbox Game Bar**

- Remapping the Xbox button only works when the **Open Game Bar using Xbox button on a controller** setting is disabled:\
  [Enable or Disable Open Game Bar using Xbox button on Controller in Windows 10](https://www.tenforums.com/tutorials/138967-enable-disable-open-xbox-game-bar-using-controller-windows-10-a.html)\
  [Enable or Disable Open Game Bar using Xbox button on Controller in Windows 11](https://www.elevenforum.com/t/enable-or-disable-open-game-bar-using-xbox-button-on-controller-in-windows-11.4290/)

- Remapping the Share button only works when the **Windows Game Recording and Broadcasting** features are disabled:\
  [Enable or Disable Windows Game Recording and Broadcasting in Windows 10](https://www.tenforums.com/tutorials/51180-enable-disable-windows-game-recording-broadcasting-windows-10-a.html)\
  [Enable or Disable Windows Game Recording and Broadcasting in Windows 11](https://www.elevenforum.com/t/enable-or-disable-game-recording-for-captures-in-windows-11.17611/)

Тестировалось на геймпаде Xbox Series X. Другие геймпады не тестировались, но по идее должны работать.


## Дорожная карта

- [ ] Запись видео c playback (obs)
- [ ] Скриншот области #2
- [ ] Редактор изображений #3
- [ ] Дополнительные языки + вычитка
  * [ ] Хинди
  * [ ] Арабский
  * [ ] Французкий
  * [ ] Бенгальский
  * [ ] Немецкий
  * [ ] Индонезийский


## Похожие проекты

- [greenshot](https://github.com/greenshot/greenshot)
- [ShareX](https://github.com/ShareX/ShareX)
- [flameshot](https://github.com/flameshot-org/flameshot)

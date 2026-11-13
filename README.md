English | [Russian](./README.ru.md) | [中文](./README.zh.md)

# ViRShot - Vibe Rust Shot

![ViRShot icon](./crates/icons/assets/png/app-128.png)

ViRShot is a cross-platform screenshot utility written in Rust

The goal is a lightweight utility that supports gamepad buttons without [Xbox Button Remapper](https://github.com/Adam777Z/xbox-controller-button-remapper). This feature apparently didn't exist in other similar projects

## Features

- Gamepad button assignment
- Filename templates, including the window title
- Saving to png, jpeg, webp
- A message is shown on all monitors when a screenshot is taken
- A yellow border around the window or screen during a screenshot - this is a quirk of how **Windows Graphics Capture (WGC)** works
- Translations into English, Russian, Simplified Chinese, Spanish, Japanese, Korean.
- Auto-update (can be disabled)
- Adding to autostart (can be disabled)
- Windows, Linux (not tested)


## Download

Download from the [releases page](https://github.com/Apkawa/virshot/releases/)

- [nightly](https://github.com/Apkawa/virshot/releases/tag/nightly) - every commit on master ends up there
- [stable](https://github.com/Apkawa/virshot/releases/latest) - stable release

## Installation and running

The program is distributed in portable mode. Unpack it into the folder you want and run it; the config will be located next to the exe.

There is no certificate, so Defender SmartScreen will show a message like "Windows protected your PC". Select "More info" and run it anyway.

## Assigning gamepad buttons

First you need to disable **Xbox Game Bar**

- Remapping the Xbox button only works when the **Open Game Bar using Xbox button on a controller** setting is disabled:\
  [Enable or Disable Open Game Bar using Xbox button on Controller in Windows 10](https://www.tenforums.com/tutorials/138967-enable-disable-open-xbox-game-bar-using-controller-windows-10-a.html)\
  [Enable or Disable Open Game Bar using Xbox button on Controller in Windows 11](https://www.elevenforum.com/t/enable-or-disable-open-game-bar-using-xbox-button-on-controller-in-windows-11.4290/)

- Remapping the Share button only works when the **Windows Game Recording and Broadcasting** features are disabled:\
  [Enable or Disable Windows Game Recording and Broadcasting in Windows 10](https://www.tenforums.com/tutorials/51180-enable-disable-windows-game-recording-broadcasting-windows-10-a.html)\
  [Enable or Disable Windows Game Recording and Broadcasting in Windows 11](https://www.elevenforum.com/t/enable-or-disable-game-recording-for-captures-in-windows-11.17611/)

Tested on an Xbox Series X gamepad. Other gamepads were not tested, but in principle they should work.


## Roadmap

- [ ] Video recording with playback (obs)
- [ ] Area screenshot #2
- [ ] Image editor #3
- [ ] Additional languages + proofreading
  * [ ] Hindi
  * [ ] Arabic
  * [ ] French
  * [ ] Bengali
  * [ ] German
  * [ ] Indonesian


## Similar projects

- [greenshot](https://github.com/greenshot/greenshot)
- [ShareX](https://github.com/ShareX/ShareX)
- [flameshot](https://github.com/flameshot-org/flameshot)

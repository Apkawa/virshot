[English](./README.md) | [Русский](./README.ru.md) | 中文

# ViRShot - Vibe Rust Shot

![ViRShot icon](./crates/icons/assets/png/app-128.png)

ViRShot — 一款用 Rust 编写的跨平台截图工具

目标是做一个轻量级的工具，无需 [Xbox Button Remapper](https://github.com/Adam777Z/xbox-controller-button-remapper) 就能支持手柄按键。其他类似的项目似乎没有这个功能

## 功能

- 手柄按键映射
- 文件名模板，其中也可以包含窗口标题
- 保存为 png、jpeg、webp
- 截图时会在所有显示器上显示提示消息
- 截图期间窗口或屏幕四周会出现黄色边框 —— 这是 **Windows Graphics Capture (WGC)** 的工作特性
- 翻译为英语、俄语、简体中文、西班牙语、日语、韩语。
- 自动更新（可关闭）
- 添加到开机自启（可关闭）
- Windows、Linux（未测试）


## 下载

从 [发布页面](https://github.com/Apkawa/virshot/releases/) 下载

- [nightly](https://github.com/Apkawa/virshot/releases/tag/nightly) - master 上的每个 commit 都会收录在这里
- [stable](https://github.com/Apkawa/virshot/releases/latest) - 稳定版发布

## 安装与运行

程序以免安装（portable）方式分发。解压到你想要的文件夹然后运行即可，配置文件会位于 exe 旁边。

没有证书，因此 Defender SmartScreen 会显示类似 "Windows protected your PC" 的提示。选择 "More info"，然后仍然运行它。

## 手柄按键映射

首先需要关闭 **Xbox Game Bar**

- 只有关闭 **Open Game Bar using Xbox button on a controller** 设置后，Xbox 按键的映射才生效：\
  [Enable or Disable Open Game Bar using Xbox button on Controller in Windows 10](https://www.tenforums.com/tutorials/138967-enable-disable-open-xbox-game-bar-using-controller-windows-10-a.html)\
  [Enable or Disable Open Game Bar using Xbox button on Controller in Windows 11](https://www.elevenforum.com/t/enable-or-disable-open-game-bar-using-xbox-button-on-controller-in-windows-11.4290/)

- 只有关闭 **Windows Game Recording and Broadcasting** 功能后，Share 按键的映射才生效：\
  [Enable or Disable Windows Game Recording and Broadcasting in Windows 10](https://www.tenforums.com/tutorials/51180-enable-disable-windows-game-recording-broadcasting-windows-10-a.html)\
  [Enable or Disable Windows Game Recording and Broadcasting in Windows 11](https://www.elevenforum.com/t/enable-or-disable-game-recording-for-captures-in-windows-11.17611/)

已在 Xbox Series X 手柄上测试过。其他手柄未测试，但理论上应该可以工作。


## 路线图

- [ ] 录制视频并支持回放（obs）
- [ ] 区域截图 #2
- [ ] 图片编辑器 #3
- [ ] 更多语言 + 校对
  * [ ] 印地语
  * [ ] 阿拉伯语
  * [ ] 法语
  * [ ] 孟加拉语
  * [ ] 德语
  * [ ] 印尼语


## 类似的项目

- [greenshot](https://github.com/greenshot/greenshot)
- [ShareX](https://github.com/ShareX/ShareX)
- [flameshot](https://github.com/flameshot-org/flameshot)

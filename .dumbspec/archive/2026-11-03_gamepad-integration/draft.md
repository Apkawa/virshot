# Запрос (2026-11-03)

1) hidpad-poc — минимальная реализация работы с кнопками геймпада.

Лог ПOC:

```
hidpad-poc build 1793728850 (2026-11-03 18:00:50 UTC) profile=debug sha=afe2bc0
gameinput: export used = GameInputCreate
gameinput: interface version chosen = v3
gameinput: focus policy set = 0x1C0 (slot 16)
registered: sysbtn=NULL, device kinds 0xE/0x40000/0x4 async, blocking Connected
gameinput: dispatcher created (v3 slot 13)
alive: devices=0, dispatch=463
device: name=Совместимое устройство ввода Bluetooth LE XINPUT vid=0x045E pid=0x0B13 usage=0x0005 supportedInput=0x1040006 supportedSystemButtons=0x3
alive: devices=1, dispatch=937
button bits=0x0001 idx=[0]
button bits=0x0000 idx=[]
button bits=0x0001 idx=[0]
button bits=0x0000 idx=[]
button bits=0x0002 idx=[1]
button bits=0x0000 idx=[]
button bits=0x0002 idx=[1]
button bits=0x0000 idx=[]
button bits=0x0008 idx=[3]
button bits=0x0000 idx=[]
button bits=0x0004 idx=[2]
button bits=0x0000 idx=[]
button bits=0x0001 idx=[0]
button bits=0x0000 idx=[]
[13438205497550318] Совместимое устройство ввода Bluetooth LE XINPUT guide DOWN
alive: devices=1, dispatch=1400
[13438205501691659] Совместимое устройство ввода Bluetooth LE XINPUT guide UP
alive: devices=1, dispatch=1862
```

Нужно скопировать в `crate/gamepad` и оформить как простую библиотеку,
с минимальными изменениями.

2) Интегрировать в virshot (биндинг клавиш геймпада и вот это всё).

Дополнение (2026-11-03): в `crate/gamepad/examples/` добавить тестовое консольное
приложение для тестирования клавиш геймпада.

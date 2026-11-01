# Draft: POC — геймпад + скриншот

**Вход от пользователя (2026-11-01, дословно):**

/dumbspec @docs/spec.md
задача сделать минимальный proof of concept
1) инициализировать rust проект, настроить сборку и установить зависимости, настроить сборку linux и windows версии
2) сделать минимальный код который будет печатать в консоль нажатия геймпада и делать скриншот при нажатии на любую кнопку геймпада
3) собрать виндовую версию, я потыкаю и потом уже двинемся дальше

у меня есть кусок кода, для примера

```rust
use gilrs::{Gilrs, Button, Event};
use std::thread;
use std::time::Duration;
use xcap::Monitor; // или screenshots

fn main() {
    // 1. Инициализация геймпада
    let mut gilrs = Gilrs::new().unwrap();

    println!("Слушаю геймпад... Нажми 'Share' или 'A' для скриншота.");

    loop {
        // 2. Опрос событий
        while let Some(Event { id, event, .. }) = gilrs.next_event() {
            match event {
                // Если нажата кнопка (например, South = 'A' на Xbox)
                gilrs::EventType::ButtonPressed(Button::South, _) => {
                    println!("Кнопка нажата! Делаю скриншот...");

                    // 3. Захват экрана в отдельном потоке, чтобы не фризить игру
                    thread::spawn(|| {
                        let monitors = Monitor::all().unwrap();
                        let primary = &monitors[0];
                        let image = primary.capture_image().unwrap();

                        // Сохранение
                        let filename = format!("screenshot_{}.png", chrono::Local::now().format("%Y%m%d_%H%M%S"));
                        image.save(&filename).unwrap();
                        println!("Сохранено: {}", filename);
                    });
                }
                _ => {}
            }
        }
        // Не грузим CPU на 100%
        thread::sleep(Duration::from_millis(10));
    }
}
```

**Контекст:** репозиторий `vibe-shot` пустой (только `docs/spec.md` — полная
спецификация VibeShot MVP). Это POC-шаг перед основной реализацией.

**Уточнения пользователя (2026-11-01, после gate step 5):**
- Триггер скриншота — **любая кнопка геймпада** (любое ButtonPressed).
- Windows-сборка — пока `cargo build --target x86_64-pc-windows-gnu`
  (пользователь качает target; MSVC-вариант разберём позже).
- Скриншоты POC сохраняются в `./.tmp/screenshots/`.

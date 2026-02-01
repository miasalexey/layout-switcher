# RU 
Переключатель раскладок клавиатуры для те кто использует 2 языка
# EN 
Keyboard layout switcher for those who use two languages

## Сделано на основе идеи https://github.com/OleksandrCEO/MagShift
Код написан Gemini, (никакого вайб-кода, просто я не умею в Rust). 

## Установка и настройка
Добавьте себя в группу input:
```Bash
sudo usermod -aG input $USER
```
### Настройка прав для /dev/uinput
Устройство /dev/uinput отвечает за эмуляцию нажатий (виртуальную клавиатуру). По умолчанию оно доступно только root.
Создайте файл правил udev:
```Bash
sudo nano /etc/udev/rules.d/99-uinput.rules
```
Вставьте туда следующую строку:
```Text
KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"
```
Это правило говорит системе: «При загрузке отдай устройство uinput группе input и разреши им чтение и запись».

### Применение изменений
Чтобы изменения вступили в силу, нужно обновить правила и переподключить пользователя к сессии.
Обновите правила udev:

```Bash
sudo udevadm control --reload-rules && sudo udevadm trigger
```
Важно: Выйдите из системы (Log out) и зайдите снова, либо просто перезагрузитесь. Группы пользователя обновляются только при новом логине.
После перезагрузки проверьте, что вы в группе:

```Bash
groups
```
(В списке должно быть слово input).


### Проверка прав доступа
Перед запуском убедитесь, что файлы устройств теперь доступны группе:
Для физических устройств: `ls -l /dev/input/event*` (должно быть brw-rw---- root input)
Для виртуального устройства: `ls -l /dev/uinput` (должно быть crw-rw---- root input)

### Systemd
```Ini
[Unit]
Description=Rust Switcher - Keyboard Layout Fixer
# Убираем сеть, она не нужна. Добавляем udev, так как мы зависим от /dev/input
After=systemd-udevd.service local-fs.target
Wants=systemd-udevd.service

[Service]
Type=simple
# Запуск от root необходим для стабильного grab()
User=root
Group=root

# Убедитесь, что бинарник и config.toml лежат именно здесь
WorkingDirectory=/usr/local/lib/rs-switcher
ExecStart=/usr/local/bin/rs-switcher

# Всегда перезапускать, если упадет
Restart=always
RestartSec=3
# Ждем завершения корректно
TimeoutStopSec=5

# Логирование
Environment="RUST_LOG=info"
StandardOutput=journal
StandardError=journal

# --- СЕКЦИЯ БЕЗОПАСНОСТИ ---
# Разрешаем только необходимые системные вызовы
CapabilityBoundingSet=CAP_SYS_ADMIN
NoNewPrivileges=yes

# Ограничиваем доступ к файловой системе
ProtectSystem=full
# Если конфиг лежит внутри WorkingDirectory, то Home не нужен
ProtectHome=yes
PrivateTmp=yes

# ВАЖНО: Доступ к устройствам
# Чтобы uinput и evdev работали, PrivateDevices должен быть "no"
PrivateDevices=no
# Дополнительно разрешаем доступ конкретно к вводу и uinput
DeviceAllow=/dev/uinput rw
DeviceAllow=char-input rw

# Защита ядра
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
MemoryDenyWriteExecute=yes
LockPersonality=yes

[Install]
# Запускать, как только система будет готова к работе пользователя
WantedBy=multi-user.target
```

Как правильно установить:
Убедитесь, что папка существует и в ней лежит конфиг:
```Bash
sudo mkdir -p /usr/local/lib/rs-switcher
sudo cp config.toml /usr/local/lib/rs-switcher/
```
Скопируйте бинарник:

```Bash
sudo cp target/release/rs-switcher /usr/local/bin/
```
Создайте файл сервиса:

```Bash
sudo nano /etc/systemd/system/rs-switcher.service
```
Примените настройки:
```Bash
sudo systemctl daemon-reload
sudo systemctl enable --now rs-switcher
```
Как проверить работу:
```Bash
# Посмотреть статус
systemctl status rs-switcher
# Посмотреть логи в реальном времени
journalctl -u rs-switcher -f
```
Если в логах появится ошибка Config not found, значит программа ищет config.toml не там, где он лежит. В этом случае проверьте путь в WorkingDirectory.

### Запуск
Теперь вы можете запускать ваш бинарник просто по имени:
```Bash
cd /path/to/your/project
./target/release/rs-switcher
```

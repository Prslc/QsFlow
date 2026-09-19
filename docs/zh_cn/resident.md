# 常驻模式

默认每次按热键都会重新拉起前端和它的 Rust 后端。要让启动器即刻弹出，让一个常驻进程
保持存活，通过 unix socket（`$XDG_RUNTIME_DIR/wayrun.sock`）切换表面：

```ini
# ~/.config/systemd/user/wayrun-launcher.service
[Unit]
Description=WayRun launcher (resident)
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=/home/you/.local/bin/wayrun
# 后端崩溃时前端以 0 退出，属于正常退出，因此不能用 on-failure
Restart=always
RestartSec=2
# 让启动器执行的命令也能看到 ~/.local/bin
Environment=PATH=/home/you/.local/bin:/usr/local/bin:/usr/bin:/bin
# WAYLAND_DISPLAY/DISPLAY 由图形会话导入；这里只设 XDG_RUNTIME_DIR（uid 无关的 %t）
Environment=XDG_RUNTIME_DIR=%t
# 常驻模式：隐藏启动，用 `wayrun toggle` 切换
Environment=WAYRUN_RESIDENT=1

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now wayrun-launcher
# niri 热键 —— 切换而非重新拉起：
#   Alt+Space { spawn-sh "wayrun toggle"; }
```

命令为 `open` / `close` / `toggle` / `status`，都发给常驻实例持有的这个 socket；
`status` 打印 `visible` 或 `hidden`。`WAYRUN_RESIDENT=1` 选中常驻模式（隐藏启动、关闭即隐藏）；
不带该变量时，直接 `wayrun` 启动即弹出、关闭即退出，因此手动/开发路径与 systemd 服务相互独立。
恢复：`systemctl --user disable --now wayrun-launcher` 并还原绑定的启动方式。

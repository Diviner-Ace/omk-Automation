#!/system/bin/sh

MODDIR="/data/adb/injector_updater"
BIN="${MODDIR}/injector_updater"

# 等待开机广播完成
until [ "$(getprop sys.boot_completed)" = "1" ]; do
    sleep 2
done

if [ -f "$BIN" ]; then
    chmod 755 "$BIN"
    killall -9 injector_updater 2>/dev/null
    # 启动后台守护进程
    "$BIN" >/dev/null 2>&1 &
fi

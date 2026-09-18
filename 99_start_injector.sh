#!/system/bin/sh

MODDIR="/data/adb/injector_updater"
BIN="${MODDIR}/injector_updater"
TARGET_DIR="/storage/emulated/0/卦师的大全/配置文件"

# 等待系统完全启动
until [ "$(getprop sys.boot_completed)" = "1" ]; do
    sleep 2
done

# 1. 启动 Rust 守护进程
if [ -f "$BIN" ]; then
    chmod 755 "$BIN"
    killall -9 injector_updater 2>/dev/null
    "$BIN" >/dev/null 2>&1 &
fi

# 2. 进入定时任务与守护循环（后台运行）
(
    while true; do
        sleep 60 # 每分钟循环一次
        
        # --- 功能A：守护进程（防止 Rust 被系统意外杀死）---
        if ! pgrep -f "injector_updater" > /dev/null; then
            "$BIN" >/dev/null 2>&1 &
        fi

        CURRENT_DATE=$(date +%Y%m%d)
        CURRENT_TIME=$(date +%H%M)
        CURRENT_TS=$(date +%s)

        # --- 功能B：每天早上 4 点重置 baseline.txt ---
        # 检查是否已经重置过，避免 4:00 到 4:59 重复重置
        if [ "$CURRENT_TIME" = "0400" ] && [ "$(cat $MODDIR/.last_reset 2>/dev/null)" != "$CURRENT_DATE" ]; then
            echo "$(date): 开始重置 baseline.txt" >> "$MODDIR/reset.log"
            
            # 1. 停止 Rust
            killall -9 injector_updater 2>/dev/null
            sleep 2
            
            # 2. 生成新快照（严格按照 Rust 逻辑：-3 减去 -s）
            pm list packages -s | sed 's/package://' > /tmp/sys.txt
            pm list packages -3 | sed 's/package://' > /tmp/user.txt
            grep -vxFf /tmp/sys.txt /tmp/user.txt > "$MODDIR/baseline.txt"
            
            # 3. 记录重置日期并重启 Rust
            echo "$CURRENT_DATE" > "$MODDIR/.last_reset"
            "$BIN" >/dev/null 2>&1 &
            echo "$(date): 重置完成，Rust 已重启" >> "$MODDIR/reset.log"
        fi

        # --- 功能C：每 15 天备份一次 ---
        LAST_BACKUP_TS=$(cat "$MODDIR/.last_backup_ts" 2>/dev/null || echo "0")
        # 15 天 = 15 * 86400 = 1296000 秒
        if [ $((CURRENT_TS - LAST_BACKUP_TS)) -ge 1296000 ]; then
            mkdir -p "$TARGET_DIR"
            cp "$MODDIR/baseline.txt" "$TARGET_DIR/baseline_$(date +%Y%m%d).txt"
            echo "$CURRENT_TS" > "$MODDIR/.last_backup_ts"
            echo "$(date): 已备份到卦师的大全" >> "$MODDIR/backup.log"
        fi
    done
) &
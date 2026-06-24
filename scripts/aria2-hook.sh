#!/bin/bash
# aria2 下载完成回调脚本 - 自动合并 B 站音视频
#
# 配置方法：
# 1. 将此脚本复制到 ~/downloads/.aria2/aria2-hook.sh
# 2. 在 ~/downloads/.aria2/aria2.conf 中添加：
#    on-download-complete=~/downloads/.aria2/aria2-hook.sh
# 3. 重启 aria2
#
# aria2 回调参数：
# $1 = GID (下载任务 ID)
# $2 = 文件数量
# $3 = 文件路径

# 获取文件路径
FILE_PATH="$3"
DIR=$(dirname "$FILE_PATH")
FILENAME=$(basename "$FILE_PATH")
EXT="${FILENAME##*.}"
STEM="${FILENAME%.*}"

# 只处理 .mp4 和 .m4a 文件
if [[ "$EXT" != "mp4" && "$EXT" != "m4a" ]]; then
    exit 0
fi

# 检查配对文件是否存在
if [[ "$EXT" == "mp4" ]]; then
    PAIR_FILE="$DIR/$STEM.m4a"
elif [[ "$EXT" == "m4a" ]]; then
    PAIR_FILE="$DIR/$STEM.mp4"
fi

# 如果配对文件不存在，等待另一个文件下载完成
if [[ ! -f "$PAIR_FILE" ]]; then
    exit 0
fi

# 检查是否有 .aria2 控制文件（表示还在下载）
if [[ -f "$FILE_PATH.aria2" || -f "$PAIR_FILE.aria2" ]]; then
    exit 0
fi

# 调用 mixbilibili 合并单个文件对
# --once: 只合并指定的 stem
# --sdel true: 合并后删除源文件
# --quiet: 静默模式，不显示进度
/Users/wenlanliu/IdeaProjects/mixbilibili/target/release/mixbilibili --once "$STEM" -s "$DIR" -o "$DIR" --sdel true --quiet

# 可选：记录日志
# echo "[$(date)] 合并完成: $STEM" >> "$DIR/.mixbilibili.log"

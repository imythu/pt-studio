#!/usr/bin/env bash
# 用 curl 构造 announce 请求跑通原型。用法：先 `cargo run` 启动，再执行本脚本。
set -u
T=${T:-http://127.0.0.1:7070}
PK=0123456789abcdef0123456789abcdef            # user 42, Active
IH=$(echo 3b245504cf5f11bbdbe1201cea6a6bf45aee1bc0 | sed 's/../%&/g')
PID=$(printf -- '-qB4630-abcdefghijkl' | od -An -tx1 | tr -d ' \n' | sed 's/../%&/g')
QB='qBittorrent/4.6.3'
ann() { # $1=passkey $2=extra-query $3=UA [$4=peer_id]
  curl -s -A "$3" "$T/announce/$1?info_hash=$IH&peer_id=${4:-$PID}&port=51413&$2" | tr -c '[:print:]' '.'; echo
}
echo "== 1. 非法 passkey";              ann not-a-passkey "uploaded=0&downloaded=0&left=100&event=started" "$QB"
echo "== 2. 未注册 passkey";            ann 00000000000000000000000000000000 "uploaded=0&downloaded=0&left=100&event=started" "$QB"
echo "== 3. 被封禁用户";                ann ffffffffffffffffffffffffffffffff "uploaded=0&downloaded=0&left=100&event=started" "$QB"
echo "== 4. 客户端不在白名单（UA 不符）"; ann $PK "uploaded=0&downloaded=0&left=100&event=started" "curl/8"
echo "== 5. 种子未注册";                curl -s -A "$QB" "$T/announce/$PK?info_hash=$(printf '%%%02x' $(seq 1 20))&peer_id=$PID&port=51413&left=1"; echo
echo "== 6. 禁止下载用户以 leecher 身份 announce"; ann aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "uploaded=0&downloaded=0&left=100&event=started" "$QB"
echo "== 7. started（首次出现）";       ann $PK "uploaded=0&downloaded=0&left=1000&event=started&compact=1" "$QB"
sleep 2
echo "== 8. 常规（up+300 down+600）";   ann $PK "uploaded=300&downloaded=600&left=400&compact=1" "$QB"
sleep 1
echo "== 9. completed";                 ann $PK "uploaded=500&downloaded=1000&left=0&event=completed&compact=1" "$QB"
sleep 1
echo "== 10. 累计值回退";               ann $PK "uploaded=100&downloaded=1000&left=0&compact=1" "$QB"
echo "== 11. 未知 peer 且非 started（模拟状态丢失）"; ann $PK "uploaded=9999&downloaded=0&left=0&compact=0" "$QB" "$(printf -- '-qB4630-zzzzzzzzzzzz' | od -An -tx1 | tr -d ' \n' | sed 's/../%&/g')"
echo "== 12. stopped";                  ann $PK "uploaded=700&downloaded=1000&left=0&event=stopped&compact=1" "$QB"
echo "== 13. 无 passkey 路径";          curl -s "$T/announce?info_hash=$IH"; echo
echo "== debug/events"; curl -s "$T/debug/events" | python3 -c 'import sys,json
d=json.load(sys.stdin); print("peer_states =", d["peer_states"])
for e in d["events"]: print(e["seq"], e["user_id"], e["event"], "up=%s down=%s left=%s"%(e["uploaded"],e["downloaded"],e["left"]), "Δ=", json.dumps(e["delta"], ensure_ascii=False))'

#!/usr/bin/env python3
"""Deluge 守护进程的一次 RPC 调用（bt-deluge.mjs 使用），结果以 JSON 输出到 stdout。

    python3 deluge-rpc.py <port> <user> <password> <method> [json-args]

例：python3 deluge-rpc.py 58900 pt pt core.get_torrents_status '[{}, ["state", "tracker_status"]]'
依赖 Deluge 2.x 自带的 Python 包（apt 的 deluged / deluge-console 已包含）。
"""

import json
import sys

from twisted.internet import defer, reactor

import deluge.log
from deluge.ui.client import client

deluge.log.setup_logger("none")
result = {}


@defer.inlineCallbacks
def main():
    port, user, password, method = sys.argv[1:5]
    args = json.loads(sys.argv[5]) if len(sys.argv) > 5 else []
    try:
        yield client.connect("127.0.0.1", int(port), user, password)
        ns, name = method.split(".", 1)
        result["ok"] = yield getattr(getattr(client, ns), name)(*args)
    except Exception as e:  # noqa: BLE001 - 原样报告给调用方
        result["error"] = repr(getattr(e, "value", e))
    finally:
        if client.connected():
            yield client.disconnect()
        reactor.stop()


reactor.callWhenRunning(main)
reactor.run()
print(json.dumps(result, default=lambda o: o.decode() if isinstance(o, bytes) else str(o)))
sys.exit(1 if "error" in result else 0)

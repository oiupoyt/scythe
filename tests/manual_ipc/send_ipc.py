import socket
import json
import struct
import os

def send(cmd):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    rt = os.environ.get("XDG_RUNTIME_DIR", "/tmp")
    sock_path = f"{rt}/scythe.sock" if os.path.exists(f"{rt}/scythe.sock") else f"{rt}/vrec.sock"
    sock.connect(sock_path)
    payload = json.dumps(cmd).encode('utf-8')
    sock.sendall(struct.pack('<I', len(payload)))
    sock.sendall(payload)

import sys
send(sys.argv[1])

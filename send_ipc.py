import socket
import json
import struct
import os

def send(cmd):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(os.environ.get("XDG_RUNTIME_DIR", "/tmp") + "/vrec.sock")
    payload = json.dumps(cmd).encode('utf-8')
    sock.sendall(struct.pack('<I', len(payload)))
    sock.sendall(payload)

import sys
send(sys.argv[1])

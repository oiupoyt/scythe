import socket
import json
import struct
import os

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect(os.environ.get("XDG_RUNTIME_DIR", "/tmp") + "/vrec.sock")
payload = b'"SaveReplay"'
sock.sendall(struct.pack('<I', len(payload)))
sock.sendall(payload)
print("Sent SaveReplay")

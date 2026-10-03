"""Disposable echo workload; used only by the VPC live data-plane test."""
import http.server
import json
import os
import socket
import threading


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        body = json.dumps({"marker": os.environ["E2E_MARKER"], "parent_key_present": "HETEROCLOUD_API_KEY" in os.environ}).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def udp():
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.bind(("0.0.0.0", 8081))
        while True:
            body, peer = connection.recvfrom(128)
            connection.sendto(body, peer)


threading.Thread(target=udp, daemon=True).start()
threading.Thread(target=http.server.ThreadingHTTPServer(("0.0.0.0", 8082), Handler).serve_forever, daemon=True).start()
http.server.ThreadingHTTPServer(("0.0.0.0", 8080), Handler).serve_forever()

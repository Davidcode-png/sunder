import time
from http.server import BaseHTTPRequestHandler, HTTPServer

class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def do_POST(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()

        events = [
            '{"choices":[{"delta":{"content":"Hello"}}]}',
            '{"choices":[{"delta":{"content":", "}}]}',
            '{"choices":[{"delta":{"content":"world"}}]}',
        ]
        for e in events:
            self.wfile.write(f"data: {e}\n\n".encode())
            self.wfile.flush()
            time.sleep(0.05)

        # send one event split awkwardly across two writes, no newline
        # in the first write, to simulate a chunk boundary mid-line
        self.wfile.write(b'data: {"choices":[{"delta":')
        self.wfile.flush()
        time.sleep(0.1)
        self.wfile.write(b'{"content":"!"}}]}\n\n')
        self.wfile.flush()

        self.wfile.write(b"data: [DONE]\n\n")
        self.wfile.flush()

    def log_message(self, *a): pass

HTTPServer(("127.0.0.1", 11434), Handler).serve_forever()
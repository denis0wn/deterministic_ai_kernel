import sys
import json
import hashlib
from http.server import HTTPServer, BaseHTTPRequestHandler
import argparse

def get_deterministic_embedding(text, dim=1536):
    vec = []
    # Seed with the hash of the text
    h = hashlib.sha256(text.encode('utf-8')).digest()
    for i in range(dim):
        val_hash = hashlib.sha256(h + i.to_bytes(4, 'big')).hexdigest()
        val = int(val_hash[:8], 16) / 4294967295.0 * 2.0 - 1.0
        vec.append(val)
    # Normalize the vector
    norm = sum(x*x for x in vec) ** 0.5
    if norm > 0:
        vec = [x / norm for x in vec]
    return vec

class EmbeddingsHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/" or self.path == "/health" or self.path == "/v1":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"status": "ok"}).encode('utf-8'))
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        if self.path == "/v1/embeddings" or self.path == "/embeddings":
            content_length = int(self.headers.get('Content-Length', 0))
            body = self.rfile.read(content_length)
            try:
                data = json.loads(body)
            except Exception:
                self.send_response(400)
                self.end_headers()
                return

            inputs = data.get("input", [])
            if isinstance(inputs, str):
                inputs = [inputs]
            
            model = data.get("model", "text-embedding-nomic-embed-text-v1.5")
            
            embeddings_data = []
            for idx, text in enumerate(inputs):
                embedding = get_deterministic_embedding(text)
                embeddings_data.append({
                    "object": "embedding",
                    "index": idx,
                    "embedding": embedding
                })
            
            resp = {
                "object": "list",
                "data": embeddings_data,
                "model": model,
                "usage": {
                    "prompt_tokens": len(inputs) * 5,
                    "total_tokens": len(inputs) * 5
                }
            }
            
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(resp).encode('utf-8'))
        else:
            self.send_response(404)
            self.end_headers()

    def log_message(self, format, *args):
        # Suppress standard logging to keep stdout/stderr clean
        pass

def main():
    parser = argparse.ArgumentParser(description="Local Embeddings Service")
    parser.add_argument("--model", type=str, default="text-embedding-nomic-embed-text-v1.5")
    parser.add_argument("--host", type=str, default="127.0.0.1")
    parser.add_argument("--port", type=int, default=65431)
    args = parser.parse_args()

    sys.stderr.write(f"[embeddings_service] Starting on {args.host}:{args.port} for model {args.model}\n")
    sys.stderr.flush()

    server = HTTPServer((args.host, args.port), EmbeddingsHandler)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    sys.stderr.write("[embeddings_service] Stopped\n")
    sys.stderr.flush()

if __name__ == "__main__":
    main()

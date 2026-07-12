import sys
import os
import socket
import json
import argparse
import traceback

def log(msg):
    sys.stderr.write(f"[mlx_native_service] {msg}\n")
    sys.stderr.flush()

def main():
    parser = argparse.ArgumentParser(description="Native MLX Inference Service")
    parser.add_argument("--model", type=str, required=True, help="Path to MLX model directory")
    parser.add_argument("--host", type=str, default="127.0.0.1", help="Host address to bind")
    parser.add_argument("--port", type=int, default=8080, help="Port to bind")
    args = parser.parse_args()

    log(f"Loading mlx_lm model from: {args.model}")
    try:
        from mlx_lm import load, generate
        model, tokenizer = load(args.model)
        log("Model and tokenizer loaded successfully.")
    except Exception as e:
        log(f"Failed to load model: {e}")
        traceback.print_exc(file=sys.stderr)
        sys.exit(1)

    server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    # Enable address reuse
    server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        server_socket.bind((args.host, args.port))
        server_socket.listen(5)
        log(f"Service listening on {args.host}:{args.port}")
    except Exception as e:
        log(f"Failed to bind to {args.host}:{args.port} - {e}")
        sys.exit(1)

    while True:
        try:
            conn, addr = server_socket.accept()
            # Set a timeout for read/write operations
            conn.settimeout(300.0)
            
            # Read until we get a full line (newline-terminated)
            data_buffer = []
            while True:
                chunk = conn.recv(4096)
                if not chunk:
                    break
                data_buffer.append(chunk)
                if b'\n' in chunk:
                    break
            
            if not data_buffer:
                conn.close()
                continue
                
            full_data = b"".join(data_buffer).decode("utf-8").strip()
            if not full_data:
                conn.close()
                continue

            try:
                request = json.loads(full_data)
            except Exception as e:
                response = {"status": "error", "error": f"Invalid JSON payload: {e}"}
                conn.sendall((json.dumps(response) + "\n").encode("utf-8"))
                conn.close()
                continue

            method = request.get("method")
            params = request.get("params", {})

            if method == "ping":
                response = {"status": "ok", "model": args.model}
                conn.sendall((json.dumps(response) + "\n").encode("utf-8"))
            elif method == "generate":
                messages = params.get("messages", [])
                temp = params.get("temp", 0.0)
                max_tokens = params.get("max_tokens", 2048)

                try:
                    # Apply chat template
                    prompt = tokenizer.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)
                    # Run generation
                    generated_text = generate(model, tokenizer, prompt=prompt, max_tokens=max_tokens)
                    response = {"status": "ok", "text": generated_text}
                except Exception as e:
                    log(f"Generation error: {e}")
                    response = {"status": "error", "error": f"Generation error: {traceback.format_exc()}"}
                
                conn.sendall((json.dumps(response) + "\n").encode("utf-8"))
            elif method == "shutdown":
                response = {"status": "ok", "message": "Shutting down"}
                conn.sendall((json.dumps(response) + "\n").encode("utf-8"))
                conn.close()
                log("Shutdown requested. Exiting.")
                break
            else:
                response = {"status": "error", "error": f"Unknown method: {method}"}
                conn.sendall((json.dumps(response) + "\n").encode("utf-8"))

            conn.close()
        except KeyboardInterrupt:
            log("Service interrupted by keyboard. Exiting.")
            break
        except Exception as e:
            log(f"Connection handling error: {e}")

    server_socket.close()

if __name__ == "__main__":
    main()

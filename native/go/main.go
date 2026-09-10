package main

import (
    "bufio"
    "encoding/json"
    "fmt"
    "os"
)

type Request struct {
    Version uint64          `json:"version"`
    Op      string          `json:"op"`
    ID      string          `json:"request_id"`
    Payload json.RawMessage `json:"payload"`
}

type Response struct {
    Version uint64      `json:"version"`
    OK      bool        `json:"ok"`
    ID      string      `json:"request_id"`
    Result  interface{} `json:"result,omitempty"`
    Error   string      `json:"error,omitempty"`
}

func main() {
    scanner := bufio.NewScanner(os.Stdin)
    scanner.Buffer(make([]byte, 4096), 256*1024)
    encoder := json.NewEncoder(os.Stdout)
    for scanner.Scan() {
        var req Request
        if err := json.Unmarshal(scanner.Bytes(), &req); err != nil {
            _ = encoder.Encode(Response{Version: 1, Error: "invalid JSON"})
            continue
        }
        if req.Version != 1 || req.ID == "" {
            _ = encoder.Encode(Response{Version: 1, ID: req.ID, Error: "invalid protocol request"})
            continue
        }
        if req.Op == "health" {
            _ = encoder.Encode(Response{Version: 1, OK: true, ID: req.ID, Result: map[string]string{"component": "go-service", "version": "0.2.0"}})
        } else {
            _ = encoder.Encode(Response{Version: 1, ID: req.ID, Error: fmt.Sprintf("unsupported operation: %s", req.Op)})
        }
    }
}

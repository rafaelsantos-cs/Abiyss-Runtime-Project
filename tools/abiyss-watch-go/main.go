package main

import (
	"bufio"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"net"
	"os"
	"time"
)

const (
	protocolVersion = 1
	maxLineBytes    = 128 * 1024
)

type request struct {
	Version uint32                 `json:"version"`
	ID      string                 `json:"id"`
	Op      string                 `json:"op"`
	Args    map[string]any         `json:"args"`
}

type response struct {
	Version uint32          `json:"version"`
	ID      string          `json:"id"`
	OK      bool            `json:"ok"`
	Result  json.RawMessage `json:"result"`
	Error   *struct {
		Code    string `json:"code"`
		Message string `json:"message"`
	} `json:"error,omitempty"`
}

func call(socketPath, op string, args map[string]any, timeout time.Duration) (json.RawMessage, error) {
	conn, err := net.DialTimeout("unix", socketPath, timeout)
	if err != nil {
		return nil, fmt.Errorf("connect system plane: %w", err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(timeout))

	id := fmt.Sprintf("watch-%d", time.Now().UnixNano())
	payload := request{Version: protocolVersion, ID: id, Op: op, Args: args}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("encode request: %w", err)
	}
	if len(encoded)+1 > maxLineBytes {
		return nil, errors.New("request exceeds configured size")
	}
	if _, err := conn.Write(append(encoded, '\n')); err != nil {
		return nil, fmt.Errorf("write request: %w", err)
	}

	reader := bufio.NewReaderSize(conn, 8192)
	line, err := reader.ReadBytes('\n')
	if err != nil {
		return nil, fmt.Errorf("read response: %w", err)
	}
	if len(line) > maxLineBytes {
		return nil, errors.New("response exceeds configured size")
	}
	var result response
	if err := json.Unmarshal(line, &result); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	if result.Version != protocolVersion {
		return nil, errors.New("protocol version mismatch")
	}
	if result.ID != id {
		return nil, errors.New("response id mismatch")
	}
	if !result.OK {
		if result.Error == nil {
			return nil, errors.New("system plane returned malformed error")
		}
		return nil, fmt.Errorf("%s: %s", result.Error.Code, result.Error.Message)
	}
	return result.Result, nil
}

func main() {
	socketPath := flag.String("socket", os.Getenv("ABIYSS_SYSTEM_SOCKET"), "ABIYSS system-plane Unix socket")
	interval := flag.Duration("interval", 0, "repeat health checks at this interval; zero means one-shot")
	flag.Parse()
	if *socketPath == "" {
		fmt.Fprintln(os.Stderr, "-socket or ABIYSS_SYSTEM_SOCKET is required")
		os.Exit(2)
	}
	check := func() error {
		raw, err := call(*socketPath, "system.info", map[string]any{}, 2*time.Second)
		if err != nil {
			return err
		}
		var info map[string]any
		if err := json.Unmarshal(raw, &info); err != nil {
			return fmt.Errorf("decode system.info result: %w", err)
		}
		encoded, _ := json.Marshal(info)
		fmt.Println(string(encoded))
		return nil
	}

	for {
		if err := check(); err != nil {
			fmt.Fprintln(os.Stderr, err)
			if *interval <= 0 {
				os.Exit(1)
			}
		}
		if *interval <= 0 {
			return
		}
		time.Sleep(*interval)
	}
}

package herdr

import (
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"runtime"
	"strings"
	"time"
)

// DeliveryUncertainError means a mutating request was written successfully but
// its response was lost. Callers must not retry the mutation automatically.
type DeliveryUncertainError struct{ Cause error }

func (e *DeliveryUncertainError) Error() string {
	return "Herdr mutation delivery is uncertain: " + e.Cause.Error()
}
func (e *DeliveryUncertainError) Unwrap() error { return e.Cause }

type rpcError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func rpcTimeout(method string) time.Duration {
	if method == "ping" {
		return 1 * time.Second
	}
	// Keep parity with the current Rust Herdr adapter for ordinary local RPCs.
	// Mutating calls must not be declared uncertain merely because a cold shell
	// or process startup crossed an overly aggressive client timeout.
	return 10 * time.Second
}

type rpcEnvelope struct {
	Result json.RawMessage `json:"result"`
	Error  *rpcError       `json:"error"`
}

func callRPC(socketPath, method string, params any, mutating bool, result any) error {
	if runtime.GOOS == "windows" {
		return errors.New("Herdr named-pipe RPC is not ported to Windows yet")
	}
	conn, err := net.DialTimeout("unix", socketPath, 400*time.Millisecond)
	if err != nil {
		return fmt.Errorf("connect Herdr socket %s: %w", socketPath, err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(rpcTimeout(method)))

	request, err := json.Marshal(map[string]any{
		"id":     "shardlane-next",
		"method": method,
		"params": params,
	})
	if err != nil {
		return err
	}
	request = append(request, '\n')
	if _, err := conn.Write(request); err != nil {
		return fmt.Errorf("write Herdr %s request: %w", method, err)
	}

	var envelope rpcEnvelope
	if err := json.NewDecoder(conn).Decode(&envelope); err != nil {
		if mutating {
			return &DeliveryUncertainError{Cause: fmt.Errorf("read %s response: %w", method, err)}
		}
		return fmt.Errorf("read Herdr %s response: %w", method, err)
	}
	if envelope.Error != nil {
		message := strings.TrimSpace(envelope.Error.Message)
		if message == "" {
			message = envelope.Error.Code
		}
		return fmt.Errorf("Herdr %s: %s", method, message)
	}
	if len(envelope.Result) == 0 || string(envelope.Result) == "null" {
		if mutating {
			return &DeliveryUncertainError{Cause: fmt.Errorf("%s response missing result", method)}
		}
		return fmt.Errorf("Herdr %s response missing result", method)
	}
	if result == nil {
		return nil
	}
	if err := json.Unmarshal(envelope.Result, result); err != nil {
		if mutating {
			return &DeliveryUncertainError{Cause: fmt.Errorf("decode %s result: %w", method, err)}
		}
		return fmt.Errorf("decode Herdr %s result: %w", method, err)
	}
	return nil
}

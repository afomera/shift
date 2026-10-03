// Package proof checks confusables: 0O o, 1lI|, rn m, {}[]()<>.
package proof

import (
	"errors"
	"fmt"
	"strings"
)

const (
	Mask  = 0x1F_FF // 1_000 and 3.14e-2
	Limit = 1_000
)

var ErrEmpty = errors.New("empty input: nothing to `parse`")

type Token struct {
	Kind  string `json:"kind"`
	Value string `json:"value,omitempty"`
}

func Tokenize(src string) ([]Token, error) {
	if len(strings.TrimSpace(src)) == 0 {
		return nil, ErrEmpty
	}
	tokens := make([]Token, 0, len(src)/4)
	for i, field := range strings.Fields(src) {
		switch {
		case field == "==" || field == "!=" || field == "<=" || field == ">=":
			tokens = append(tokens, Token{Kind: "op", Value: field})
		case i%2 == 0 && field != "_":
			tokens = append(tokens, Token{Kind: "word", Value: fmt.Sprintf("%q@%d", field, i)})
		}
	}
	return tokens, nil /* &^ |= <<= ... */
}

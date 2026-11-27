package main

import (
	"errors"
	"net/url"
	"strings"

	"rsc.io/qr"
)

const (
	quiet   = 4  // light modules around the code, as the QR standard asks
	maxRows = 64 // the pixels node's limit
)

// pairPayload is what the QR code carries: the tunnel URL and a one-time code.
func pairPayload(tunnel, code string) string {
	return "wai-remoto:pair?url=" + url.QueryEscape(tunnel) + "&code=" + code
}

// qrRows draws payload as pixels rows: '#' for a dark module, ' ' for a light one, level M, with
// the quiet zone. A payload too long for 64 rows is an error, never a cut code.
func qrRows(payload string) ([]string, error) {
	c, err := qr.Encode(payload, qr.M)
	if err != nil {
		return nil, err
	}
	n := c.Size + 2*quiet
	if n > maxRows {
		return nil, errors.New("The tunnel URL is too long for a QR code")
	}
	rows := make([]string, n)
	for y := range rows {
		var b strings.Builder
		for x := 0; x < n; x++ {
			if c.Black(x-quiet, y-quiet) {
				b.WriteByte('#')
			} else {
				b.WriteByte(' ')
			}
		}
		rows[y] = b.String()
	}
	return rows, nil
}

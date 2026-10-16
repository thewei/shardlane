package terminal

// osc7501.go watches the program's output byte stream for OSC 7501, the
// Program Status Protocol (spec rev 0.4). The emulator core (libghostty-vt)
// ignores the sequence as an unknown OSC; this scanner runs beside it on the
// same bytes, so an embedding app can consume status reports through
// Options.OnProgramStatus and the terminal can answer the protocol's
// feature-detection query. It knows the framing only — OSC through ST, the
// pairs body between them — and never touches rendering or the screen.

// osc7501Reply answers the feature-detection query `OSC 7501 ; ? ST` with
// the full fixed state and kind lists the spec defines.
const osc7501Reply = "\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\"

// osc7501Max is the spec's hard cap on one whole sequence, OSC through ST.
const osc7501Max = 4096

// Scanner states. afterESC holds across one escape byte so that both the
// string terminator (ESC \) and a canceling escape inside a body resolve
// without losing bytes that begin a new sequence.
const (
	oscGround   = iota // outside any escape sequence
	oscAfterESC        // saw ESC; the next byte decides
	oscNum             // inside `ESC ]`, collecting the OSC number
	oscSkip            // inside an OSC that is not 7501; riding to its end
	oscBody            // inside `ESC ] 7501 ;`, collecting the body
	oscBodyESC         // saw ESC inside the body: ST, a cancel, or a new sequence
)

// osc7501Scanner finds complete OSC 7501 sequences in a chunked byte
// stream. Sequences split across feed calls are handled; a body without a
// terminator within the cap is dropped whole.
type osc7501Scanner struct {
	state int
	num   []byte
	body  []byte
}

// feed consumes one chunk and returns the complete bodies seen in it,
// already stripped of `7501;`. A returned nil with query true means the
// feature-detection query `OSC 7501 ; ? ST`.
func (s *osc7501Scanner) feed(p []byte) (bodies [][]byte, query bool) {
	for _, c := range p {
		switch s.state {
		case oscGround:
			if c == 0x1b {
				s.state = oscAfterESC
			}
		case oscAfterESC:
			switch c {
			case 0x1b: // ESC ESC: still waiting on the deciding byte
			case ']':
				s.state = oscNum
				s.num = s.num[:0]
			default:
				s.state = oscGround
			}
		case oscNum:
			switch {
			case c >= '0' && c <= '9':
				if len(s.num) < 8 {
					s.num = append(s.num, c)
				}
			case c == ';' && string(s.num) == "7501":
				s.state = oscBody
				s.body = s.body[:0]
			case c == ';':
				s.state = oscSkip
			case c == 0x07: // BEL with no body: done
				s.state = oscGround
			case c == 0x1b:
				s.state = oscAfterESC
			default:
				s.state = oscSkip
			}
		case oscSkip:
			switch c {
			case 0x07:
				s.state = oscGround
			case 0x1b:
				s.state = oscAfterESC
			}
		case oscBody:
			switch c {
			case 0x07:
				bodies, query = s.emit(bodies, query)
			case 0x1b:
				s.state = oscBodyESC
			default:
				if len(s.body) < osc7501Max {
					s.body = append(s.body, c)
				} else { // over the cap: dropped whole, ride to the end
					s.state = oscSkip
				}
			}
		case oscBodyESC:
			// The byte after ESC decides: ST completes the pending body;
			// anything else abandoned it and starts fresh, so `]` opens a
			// new OSC exactly as it would from the ground state.
			switch c {
			case '\\': // ST: the sequence is complete
				bodies, query = s.emit(bodies, query)
			case 0x1b: // ESC ESC: still waiting on the deciding byte
			case ']':
				s.state = oscNum
				s.num = s.num[:0]
			default:
				s.state = oscGround
			}
		}
	}
	return bodies, query
}

// emit closes the collected body. The feature query never appears among
// the bodies.
func (s *osc7501Scanner) emit(bodies [][]byte, query bool) ([][]byte, bool) {
	s.state = oscGround
	if len(s.body) == 1 && s.body[0] == '?' {
		return bodies, true
	}
	if len(s.body) > 0 {
		bodies = append(bodies, append([]byte(nil), s.body...))
	}
	return bodies, query
}

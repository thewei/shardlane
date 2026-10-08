package raster

import (
	"sync"
	"sync/atomic"
	"time"
)

// A team does a piece of work on several cores: the goroutine asking for
// it, and helpers waiting for work, started the first time they are needed
// and kept, shared by every team. Its parts are taken in turn, as they
// differ in cost. A frame drawn on several cores so allocates nothing, as
// goroutines started for each area did (a closure each, and what they
// shared).
type team struct {
	// do does a part of the work as a member, 0 for the goroutine asking:
	// set once, as a method value allocates. It must not run a team.
	do func(member, part int)

	parts int
	next  atomic.Int32
	busy  atomic.Int64
	wg    sync.WaitGroup
	// shares are what the helpers of a run get, a member each.
	shares [maxWorkers]share
}

type share struct {
	t      *team
	member int
}

var (
	// helped are the shares of runs, which helpers take.
	helped  = make(chan *share, maxWorkers)
	helpers atomic.Int32
	hiring  sync.Mutex
)

// run does parts parts of the work on members cores at most, and returns
// how long the cores took, together: on several cores, a multiple of how
// long it lasted.
func (t *team) run(members, parts int) time.Duration {
	members = max(min(members, parts, maxWorkers), 1)
	t.parts = parts
	t.next.Store(0)
	t.busy.Store(0)
	hire(members - 1)
	t.wg.Add(members - 1)
	for m := 1; m < members; m++ {
		t.shares[m] = share{t, m}
		helped <- &t.shares[m]
	}
	t.work(0)
	t.wg.Wait()
	return time.Duration(t.busy.Load())
}

// work does parts until none is left.
func (t *team) work(member int) {
	began := time.Now()
	for p := int(t.next.Add(1)) - 1; p < t.parts; p = int(t.next.Add(1)) - 1 {
		t.do(member, p)
	}
	t.busy.Add(int64(time.Since(began)))
}

// hire starts helpers until there are n.
func hire(n int) {
	if int(helpers.Load()) >= n {
		return
	}
	hiring.Lock()
	defer hiring.Unlock()
	for int(helpers.Load()) < n {
		helpers.Add(1)
		go help()
	}
}

// help does the shares of runs, as long as the program runs.
func help() {
	for s := range helped {
		s.t.work(s.member)
		s.t.wg.Done()
	}
}

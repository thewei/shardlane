package nativeui

import (
	"log/slog"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

// Remote machine presentation state. Saved SSH machines are Herdr's own
// remote-connection machinery; this file only renders them in the Session
// selector and drives herdr's sanctioned management commands. The profile
// list lives in Herdr's client state — never a Shardlane-side registry.

const machineStatusRetryWindow = 30 * time.Second

// machineMenuSuffix renders the availability suffix for one machine row in
// the selector menu. Plain text: platform menus carry no styling.
func machineMenuSuffix(machine *herdr.Machine, state herdr.MachineState, known bool) string {
	if machine != nil && !machine.Enabled {
		return " — disabled"
	}
	if !known {
		return ""
	}
	switch {
	case state.Reachable:
		return " — reachable"
	case state.Attention && state.Message != "":
		message := []rune(state.Message)
		if len(message) > 48 {
			message = append(message[:45], '…')
		}
		return " — " + string(message)
	case state.Attention:
		return " — needs attention"
	default:
		return " — unreachable"
	}
}

// machineInstances returns the saved-machine entries of the instance list.
func (s *Shell) machineInstances() []herdr.Instance {
	machines := make([]herdr.Instance, 0, 2)
	for _, instance := range s.instances {
		if instance.Machine != nil {
			machines = append(machines, instance)
		}
	}
	return machines
}

// activeIsRemote reports whether the active instance belongs to a remote
// machine. Local-folder workflows (New Workspace from Folder, local file
// panels) are meaningless against remote instance paths.
func (s *Shell) activeIsRemote() bool {
	active := s.activeInstanceInfo()
	return active != nil && active.Machine != nil
}

// machineSuffix looks up the rendered suffix for one machine row.
func (s *Shell) machineSuffix(instance herdr.Instance) string {
	state, known := s.machineStates[instance.Machine.ID]
	return machineMenuSuffix(instance.Machine, state, known)
}

// refreshMachineStatus runs one throttled background availability check for
// all saved machines (fresh SSH round trips, so seconds). Menu opens and the
// machine flows trigger it; results only refresh the suffix rendering.
func (s *Shell) refreshMachineStatus(force bool) {
	if s.win == nil {
		return
	}
	if !force {
		if s.machineStatusRunning || time.Since(s.machineStatusAt) < machineStatusRetryWindow {
			return
		}
		if len(s.machineInstances()) == 0 {
			return
		}
	}
	s.machineStatusRunning = true
	go func() {
		states, err := s.runtime.MachineStates()
		s.applyOnUI(func() {
			s.machineStatusRunning = false
			s.machineStatusAt = time.Now()
			if err != nil {
				slog.Warn("machine status check failed", "error", err)
				return
			}
			s.machineStates = states
		})
	}()
}

// addRemoteMachine opens `herdr machine add` in Terminal.app; the setup is
// interactive by design (session discovery, server install/replace approval).
func (s *Shell) addRemoteMachine(target string) {
	s.status = "Adding remote machine — finish the setup in the opened Terminal window"
	slog.Info("add remote machine requested", "target", target)
	go func() {
		err := s.runtime.OpenMachineAddInTerminal(target)
		s.applyOnUI(func() {
			if err != nil {
				s.status = err.Error()
				return
			}
			s.status = "Complete the Herdr machine setup in Terminal, then reopen the Session menu"
			// One delayed refresh picks up the profile if setup finished
			// quickly; menu opens keep re-checking (throttled) regardless.
			go func() {
				time.Sleep(20 * time.Second)
				s.applyOnUI(func() {
					s.reloadInstances(false)
					s.refreshMachineStatus(true)
				})
			}()
		})
	}()
}

// renameMachine renames a saved machine profile's display label.
func (s *Shell) renameMachine(id, label string) {
	if s.win == nil || s.loading {
		return
	}
	generation := s.generation.Add(1)
	s.loading = true
	s.status = "Renaming remote machine…"
	go func() {
		if err := s.runtime.RenameMachine(id, label); err != nil {
			s.updateError(generation, err)
			return
		}
		instances, err := s.runtime.ListAllInstances()
		if err != nil {
			s.updateError(generation, err)
			return
		}
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			s.instances = instances
			s.loading = false
			s.errText = ""
			s.status = ""
		})
	}()
}

// forgetMachine removes a saved machine profile. Only that machine
// disconnects; the remote session keeps running on its own device.
func (s *Shell) forgetMachine(id string) {
	if s.win == nil || s.loading {
		return
	}
	generation := s.generation.Add(1)
	s.loading = true
	s.status = "Forgetting remote machine…"
	active := s.activeInstance
	go func() {
		if err := s.runtime.RemoveMachine(id); err != nil {
			s.updateError(generation, err)
			return
		}
		instances, err := s.runtime.ListAllInstances()
		if err != nil {
			s.updateError(generation, err)
			return
		}
		s.win.Update(func() {
			if generation != s.generation.Load() {
				return
			}
			if active != "" && active == herdr.MachineInstanceKey(id) {
				if s.watchCancel != nil {
					s.watchCancel()
					s.watchCancel = nil
				}
				s.closeAllTerminals()
				s.activeInstance = ""
				s.projection = herdr.Projection{}
				s.clearLocalSelection()
				s.selected = ""
			}
			delete(s.machineStates, id)
			s.instances = instances
			s.loading = false
			s.errText = ""
			s.status = "Choose a Session"
		})
	}()
}

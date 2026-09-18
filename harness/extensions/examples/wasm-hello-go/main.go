// The same tool as wasm-hello-rs, in TinyGo, against the raw WIT.
//
// No Orrery SDK: the arena is built by hand here, which is exactly what this
// example is for. A guest language with no SDK of ours must still be able to
// produce a surface the kernel accepts, and the shape below is the frozen one
// from plan 14 — parent first, children strictly after their parent, and only
// a stack or a custom carrying children at all.
package main

import (
	"fmt"

	"go.bytecodealliance.org/cm"

	surfaces "github.com/kouji-dev/orrery/examples/wasm-hello-go/internal/orrery/extension/surfaces"
	tools "github.com/kouji-dev/orrery/examples/wasm-hello-go/internal/orrery/extension/tools"
)

func init() {
	tools.Exports.Call = call
}

// jsonString escapes a string for the payload. No encoding/json: TinyGo's
// reflection-heavy paths cost component bytes this example does not need.
func jsonString(s string) string {
	out := []rune{'"'}
	for _, c := range s {
		switch c {
		case '"':
			out = append(out, '\', '"')
		case '\':
			out = append(out, '\', '\')
		case '\n':
			out = append(out, '\', 'n')
		default:
			out = append(out, c)
		}
	}
	return string(append(out, '"'))
}

func node(kind surfaces.NodeKind, payload string, children []uint32) surfaces.SurfaceNode {
	return surfaces.SurfaceNode{
		Kind:     kind,
		ID:       cm.None[string](),
		Status:   cm.None[surfaces.NodeStatus](),
		Payload:  payload,
		Children: cm.ToList(children),
	}
}

func call(name string, input string) (result cm.Result[surfaces.Surface, surfaces.Surface, string]) {
	switch name {
	case "hello":
		who := input
		if who == "" {
			who = "world"
		}
		// Index 0 is the stack; 1 and 2 are its children. Both are greater
		// than 0, which is the invariant the host checks on arrival.
		nodes := []surfaces.SurfaceNode{
			node(surfaces.NodeKindStack,
				`{"t":"stack","dir":"column","collapsed":false,"title":"hello"}`,
				[]uint32{1, 2}),
			node(surfaces.NodeKindText,
				fmt.Sprintf(`{"t":"text","value":%s}`, jsonString("hello, "+who)),
				nil),
			node(surfaces.NodeKindTable,
				`{"t":"table","columns":["language","sdk"],`+
					`"rows":[[{"text":"go"},{"text":"wit-bindgen"}]]}`,
				nil),
		}
		return cm.OK[cm.Result[surfaces.Surface, surfaces.Surface, string]](
			surfaces.Surface{Nodes: cm.ToList(nodes), Root: 0})
	default:
		return cm.Err[cm.Result[surfaces.Surface, surfaces.Surface, string]](
			"no such tool: " + name)
	}
}

func main() {}

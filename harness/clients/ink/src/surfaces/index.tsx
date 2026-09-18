/**
 * One component per core surface, and the switch that picks between them.
 *
 * Task 6's point: the switch's default arm is typed `never`, so adding a
 * variant to the generated `SurfaceKind` is a compile error here rather than a
 * blank hole in somebody's transcript. `COMPONENTS` is a mapped type over the
 * same tags, so a new variant also fails as a missing key.
 */

import { Text } from "ink";
import type { ComponentType, ReactElement } from "react";

import type { SurfaceKind } from "@orrery/protocol";

import type { Drawable, SurfaceProps } from "./kinds.js";

export type { Drawable, Intent, KindOf, SurfaceProps } from "./kinds.js";
export { drawable } from "./kinds.js";

/** Placeholder until the component for this kind lands (task 3). */
function NotDrawn<T extends SurfaceKind["t"]>({ node }: SurfaceProps<T>): ReactElement {
  return <Text color="red">[no renderer for {node.kind.t}]</Text>;
}

/** Every kind, by tag. A missing key is a type error. */
export const COMPONENTS: {
  [T in SurfaceKind["t"]]: ComponentType<SurfaceProps<T>>;
} = {
  text: NotDrawn,
  markdown: NotDrawn,
  table: NotDrawn,
  tree: NotDrawn,
  diff: NotDrawn,
  progress: NotDrawn,
  stream: NotDrawn,
  task: NotDrawn,
  question: NotDrawn,
  form: NotDrawn,
  stack: NotDrawn,
  custom: NotDrawn,
};

/** Draw one surface. */
export function SurfaceNode(props: SurfaceProps): ReactElement {
  const kind = props.node.kind;
  const node = props.node as Drawable;
  switch (kind.t) {
    case "text": {
      const C = COMPONENTS.text;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "markdown": {
      const C = COMPONENTS.markdown;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "table": {
      const C = COMPONENTS.table;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "tree": {
      const C = COMPONENTS.tree;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "diff": {
      const C = COMPONENTS.diff;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "progress": {
      const C = COMPONENTS.progress;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "stream": {
      const C = COMPONENTS.stream;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "task": {
      const C = COMPONENTS.task;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "question": {
      const C = COMPONENTS.question;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "form": {
      const C = COMPONENTS.form;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "stack": {
      const C = COMPONENTS.stack;
      return <C {...props} node={{ ...node, kind }} />;
    }
    case "custom": {
      const C = COMPONENTS.custom;
      return <C {...props} node={{ ...node, kind }} />;
    }
    default: {
      // Adding a variant to `SurfaceKind` fails to compile right here.
      const unreachable: never = kind;
      return <Text color="red">[unknown surface {JSON.stringify(unreachable)}]</Text>;
    }
  }
}

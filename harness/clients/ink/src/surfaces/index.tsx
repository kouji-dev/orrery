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

import { CustomSurface } from "./Custom.js";
import { DiffSurface } from "./Diff.js";
import { FormSurface } from "./Form.js";
import { MarkdownSurface } from "./Markdown.js";
import { ProgressSurface } from "./Progress.js";
import { QuestionSurface } from "./Question.js";
import { StackSurface } from "./Stack.js";
import { StreamSurface } from "./Stream.js";
import { TableSurface } from "./Table.js";
import { TaskSurface } from "./Task.js";
import { TextSurface } from "./Text.js";
import { TreeSurface } from "./Tree.js";
import type { Drawable, SurfaceProps } from "./kinds.js";

export type { Drawable, Intent, KindOf, SurfaceProps } from "./kinds.js";
export { drawable } from "./kinds.js";

/** Every kind, by tag. A missing key is a type error. */
export const COMPONENTS: {
  [T in SurfaceKind["t"]]: ComponentType<SurfaceProps<T>>;
} = {
  text: TextSurface,
  markdown: MarkdownSurface,
  table: TableSurface,
  tree: TreeSurface,
  diff: DiffSurface,
  progress: ProgressSurface,
  stream: StreamSurface,
  task: TaskSurface,
  question: QuestionSurface,
  form: FormSurface,
  stack: StackSurface,
  custom: CustomSurface,
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

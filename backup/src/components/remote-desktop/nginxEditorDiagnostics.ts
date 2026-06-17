import type { Extension } from '@codemirror/state';
import { RangeSetBuilder } from '@codemirror/state';
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';

export interface NginxEditorDiagnostic {
  line: number;
  fromColumn: number;
  toColumn: number;
  severity: 'error' | 'warning';
  message: string;
}

interface PendingStatement {
  line: number;
  fromColumn: number;
  toColumn: number;
  name: string;
}

interface OpenBlock {
  line: number;
  column: number;
  name: string;
  hasContent: boolean;
}

const nginxBlockDirectives = new Set([
  'events',
  'geo',
  'http',
  'if',
  'limit_except',
  'location',
  'map',
  'match',
  'server',
  'stream',
  'types',
  'upstream',
]);

function getFirstWord(value: string) {
  return value.trimStart().match(/^[^\s{};#]+/u)?.[0] ?? '';
}

function markParentHasContent(stack: OpenBlock[]) {
  const parent = stack[stack.length - 1];
  if (parent) parent.hasContent = true;
}

function createDiagnostic(
  line: number,
  fromColumn: number,
  toColumn: number,
  severity: NginxEditorDiagnostic['severity'],
  message: string,
): NginxEditorDiagnostic {
  const safeFrom = Math.max(1, fromColumn);
  const safeTo = Math.max(safeFrom + 1, toColumn);
  return { line, fromColumn: safeFrom, toColumn: safeTo, severity, message };
}

export function analyzeNginxConfig(content: string): NginxEditorDiagnostic[] {
  const diagnostics: NginxEditorDiagnostic[] = [];
  const stack: OpenBlock[] = [];
  const lines = content.split(/\r?\n/u);
  let pending: PendingStatement | null = null;

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
    const line = lines[lineIndex];
    const lineNumber = lineIndex + 1;
    let quote: string | null = null;
    let escaped = false;

    for (let index = 0; index < line.length; index += 1) {
      const char = line[index];
      const column = index + 1;

      if (quote) {
        if (escaped) {
          escaped = false;
          continue;
        }
        if (char === '\\') {
          escaped = true;
          continue;
        }
        if (char === quote) {
          quote = null;
        }
        continue;
      }

      if (char === '"' || char === "'") {
        quote = char;
        if (!pending) {
          pending = {
            line: lineNumber,
            fromColumn: column,
            toColumn: Math.max(column + 1, line.length + 1),
            name: getFirstWord(line.slice(index)),
          };
          markParentHasContent(stack);
        }
        continue;
      }

      if (char === '#') break;
      if (/\s/u.test(char)) continue;

      if (char === '{') {
        if (!pending) {
          diagnostics.push(createDiagnostic(lineNumber, column, column + 1, 'error', 'Unexpected block opener.'));
          stack.push({ line: lineNumber, column, name: '', hasContent: false });
          continue;
        }

        markParentHasContent(stack);
        stack.push({ line: lineNumber, column, name: pending.name, hasContent: false });
        pending = null;
        continue;
      }

      if (char === ';') {
        if (pending && nginxBlockDirectives.has(pending.name)) {
          diagnostics.push(createDiagnostic(
            pending.line,
            pending.fromColumn,
            pending.toColumn,
            'warning',
            'Block directive should use "{ ... }" instead of ";".',
          ));
        }
        if (pending) markParentHasContent(stack);
        pending = null;
        continue;
      }

      if (char === '}') {
        if (pending) {
          diagnostics.push(createDiagnostic(
            pending.line,
            pending.fromColumn,
            pending.toColumn,
            'error',
            'Directive is missing a trailing ";" or "{".',
          ));
          pending = null;
        }

        const openBlock = stack.pop();
        if (!openBlock) {
          diagnostics.push(createDiagnostic(lineNumber, column, column + 1, 'error', 'Unexpected closing brace.'));
          continue;
        }

        if (!openBlock.hasContent) {
          diagnostics.push(createDiagnostic(
            openBlock.line,
            openBlock.column,
            openBlock.column + 1,
            'warning',
            `${openBlock.name || 'Block'} block is empty.`,
          ));
        }
        markParentHasContent(stack);
        continue;
      }

      if (!pending) {
        pending = {
          line: lineNumber,
          fromColumn: column,
          toColumn: Math.max(column + 1, line.length + 1),
          name: getFirstWord(line.slice(index)),
        };
        markParentHasContent(stack);
      }
    }

    if (quote) {
      diagnostics.push(createDiagnostic(lineNumber, Math.max(1, line.length), line.length + 1, 'warning', 'Unclosed quote.'));
    }
  }

  if (pending) {
    diagnostics.push(createDiagnostic(
      pending.line,
      pending.fromColumn,
      pending.toColumn,
      'error',
      'Directive is missing a trailing ";" or "{".',
    ));
  }

  for (let index = stack.length - 1; index >= 0; index -= 1) {
    const openBlock = stack[index];
    diagnostics.push(createDiagnostic(openBlock.line, openBlock.column, openBlock.column + 1, 'error', 'Unclosed block.'));
  }

  return diagnostics;
}

function buildDiagnosticDecorations(view: EditorView) {
  const builder = new RangeSetBuilder<Decoration>();
  const diagnostics = analyzeNginxConfig(view.state.doc.toString()).sort((left, right) => (
    left.line - right.line || left.fromColumn - right.fromColumn
  ));

  diagnostics.forEach((diagnostic) => {
    const line = view.state.doc.line(Math.min(diagnostic.line, view.state.doc.lines));
    const from = Math.min(line.to, line.from + diagnostic.fromColumn - 1);
    const to = Math.min(view.state.doc.length, Math.max(from + 1, line.from + diagnostic.toColumn - 1));
    builder.add(from, to, Decoration.mark({
      attributes: { title: diagnostic.message },
      class: `nginx-editor-diagnostic nginx-editor-diagnostic-${diagnostic.severity}`,
    }));
  });

  return builder.finish();
}

export const nginxEditorDiagnosticsExtension: Extension = ViewPlugin.fromClass(class {
  decorations: DecorationSet;

  constructor(view: EditorView) {
    this.decorations = buildDiagnosticDecorations(view);
  }

  update(update: ViewUpdate) {
    if (update.docChanged) {
      this.decorations = buildDiagnosticDecorations(update.view);
    }
  }
}, {
  decorations: (value) => value.decorations,
});

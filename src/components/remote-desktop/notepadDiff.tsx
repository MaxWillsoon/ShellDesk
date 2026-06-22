import { t, type AppLanguage } from '../../i18n';
import type { DiffPreview, DiffPreviewLine } from './notepadTypes';

const MAX_DIFF_INPUT_LINES = 180;
const MAX_DIFF_OUTPUT_LINES = 280;

function normalizeDiffLines(content: string): string[] {
  return content.replace(/\r\n/g, '\n').replace(/\r/g, '\n').split('\n');
}

export function buildDiffPreview(beforeContent: string, afterContent: string, language: AppLanguage): DiffPreview {
  const beforeLines = normalizeDiffLines(beforeContent);
  const afterLines = normalizeDiffLines(afterContent);
  const beforeSample = beforeLines.slice(0, MAX_DIFF_INPUT_LINES);
  const afterSample = afterLines.slice(0, MAX_DIFF_INPUT_LINES);
  const lcs = Array.from(
    { length: beforeSample.length + 1 },
    () => Array<number>(afterSample.length + 1).fill(0),
  );

  for (let beforeIndex = beforeSample.length - 1; beforeIndex >= 0; beforeIndex -= 1) {
    for (let afterIndex = afterSample.length - 1; afterIndex >= 0; afterIndex -= 1) {
      lcs[beforeIndex][afterIndex] = beforeSample[beforeIndex] === afterSample[afterIndex]
        ? lcs[beforeIndex + 1][afterIndex + 1] + 1
        : Math.max(lcs[beforeIndex + 1][afterIndex], lcs[beforeIndex][afterIndex + 1]);
    }
  }

  const lines: DiffPreviewLine[] = [];
  let beforeIndex = 0;
  let afterIndex = 0;

  while (beforeIndex < beforeSample.length || afterIndex < afterSample.length) {
    if (
      beforeIndex < beforeSample.length
      && afterIndex < afterSample.length
      && beforeSample[beforeIndex] === afterSample[afterIndex]
    ) {
      lines.push({ kind: 'context', text: beforeSample[beforeIndex] });
      beforeIndex += 1;
      afterIndex += 1;
    } else if (
      afterIndex < afterSample.length
      && (
        beforeIndex >= beforeSample.length
        || lcs[beforeIndex][afterIndex + 1] >= lcs[beforeIndex + 1][afterIndex]
      )
    ) {
      lines.push({ kind: 'added', text: afterSample[afterIndex] });
      afterIndex += 1;
    } else {
      lines.push({ kind: 'removed', text: beforeSample[beforeIndex] });
      beforeIndex += 1;
    }

    if (lines.length >= MAX_DIFF_OUTPUT_LINES) break;
  }

  const truncated = (
    beforeLines.length > beforeSample.length
    || afterLines.length > afterSample.length
    || beforeIndex < beforeSample.length
    || afterIndex < afterSample.length
  );

  if (truncated) {
    lines.push({ kind: 'meta', text: t('notepad.diff.preview.truncated', language) });
  }

  if (lines.length === 0) {
    lines.push({ kind: 'meta', text: t('notepad.diff.preview.noChanges', language) });
  }

  return { lines, truncated };
}

function getDiffPrefix(kind: DiffPreviewLine['kind']): string {
  if (kind === 'added') return '+';
  if (kind === 'removed') return '-';
  if (kind === 'meta') return '!';
  return ' ';
}

export function NotepadDiffPreview({ preview, language }: { preview: DiffPreview; language: AppLanguage }) {
  return (
    <div className="notepad-diff-preview" aria-label={t('notepad.diff.preview.aria', language)}>
      {preview.lines.map((line, index) => (
        <div key={`${line.kind}-${index}`} className={`notepad-diff-line ${line.kind}`}>
          <span className="notepad-diff-prefix">{getDiffPrefix(line.kind)}</span>
          <span className="notepad-diff-text">{line.text || ' '}</span>
        </div>
      ))}
    </div>
  );
}

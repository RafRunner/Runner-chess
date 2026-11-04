export type Cell = string | number | undefined;

/**
 * A table for the terminal: each column as wide as its widest cell, numbers to the right
 * with thousands separated, and nothing for `undefined`. The columns of `right` go to
 * the right too, for numbers already written with their units.
 */
export function table(headers: string[], rows: Cell[][], right: string[] = []): string {
  const text = (cell: Cell) =>
    cell === undefined ? '' : typeof cell === 'number' ? cell.toLocaleString('en-US') : cell;
  const lines = [headers, ...rows.map((row) => row.map(text))];
  const widths = headers.map((_, i) => Math.max(...lines.map((line) => (line[i] ?? '').length)));
  const toTheRight = headers.map(
    (header, i) => right.includes(header) || rows.some((row) => typeof row[i] === 'number'),
  );
  const format = (line: string[]) =>
    line
      .map((cell, i) => (toTheRight[i] ? cell.padStart(widths[i]) : cell.padEnd(widths[i])))
      .join('  ')
      .trimEnd();
  const rule = widths.map((width) => '─'.repeat(width)).join('  ');
  return [format(lines[0]), rule, ...lines.slice(1).map(format)].join('\n');
}

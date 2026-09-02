// 对齐 C:/tmp/opencut-classic/apps/web/src/core/managers/commands.ts 的历史栈
// 本仓适配：selection 机制 stub，不依赖 EditorCore 的 selection manager。
import type { Command, CommandResult } from "./base";

interface HistoryEntry {
  command: Command;
  selectionBefore?: unknown;
  selectionOverride?: unknown;
}

export class CommandHistory {
  isRippleEnabled = false;
  private readonly history: HistoryEntry[] = [];
  private readonly redoStack: HistoryEntry[] = [];

  execute({ command }: { command: Command }): CommandResult | undefined {
    const result = command.execute();
    this.history.push({ command });
    this.redoStack.length = 0;
    void result;
    return result;
  }

  undo(): void {
    const entry = this.history.pop();
    if (!entry) return;
    entry.command.undo();
    this.redoStack.push(entry);
  }

  redo(): CommandResult | undefined {
    const entry = this.redoStack.pop();
    if (!entry) return undefined;
    const result = entry.command.redo();
    this.history.push(entry);
    return result;
  }

  canUndo(): boolean {
    return this.history.length > 0;
  }

  canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  clear(): void {
    this.history.length = 0;
    this.redoStack.length = 0;
  }
}

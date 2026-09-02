export type CommandSelectionPatch = unknown;

export interface CommandResult {
  selection?: CommandSelectionPatch;
}

export abstract class Command {
  abstract execute(): CommandResult | undefined;
  undo(): void {
    throw new Error("Undo not implemented for this command");
  }
  redo(): CommandResult | undefined {
    return this.execute();
  }
}

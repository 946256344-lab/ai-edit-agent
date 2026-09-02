import { Command, type CommandResult } from "./base";

export class BatchCommand extends Command {
  private readonly commands: Command[];
  constructor(commands: Command[]) {
    super();
    this.commands = commands;
  }

  execute(): CommandResult | undefined {
    let latest: CommandResult | undefined;
    for (const c of this.commands) {
      const r = c.execute();
      if (r?.selection !== undefined) latest = r;
    }
    return latest;
  }

  undo(): void {
    for (const c of [...this.commands].reverse()) c.undo();
  }

  redo(): CommandResult | undefined {
    let latest: CommandResult | undefined;
    for (const c of this.commands) {
      const r = c.redo();
      if (r?.selection !== undefined) latest = r;
    }
    return latest;
  }
}

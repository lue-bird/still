import * as vscode from "vscode";
import * as language_client_node from "vscode-languageclient/node";
import * as child_process from "node:child_process";

// switch to your locally built debug executable path when developing
const languageServerExecutableName: string = "still";
let client: language_client_node.LanguageClient | null = null;
export async function activate(context: vscode.ExtensionContext): Promise<void> {
  client = new language_client_node.LanguageClient(
    "still",
    "still",
    async () => child_process.spawn(languageServerExecutableName),
    {
      diagnosticCollectionName: "still",
      documentSelector: [{ scheme: "file", language: "still" }],
      synchronize: {
        fileEvents: vscode.workspace.createFileSystemWatcher("**/*.still")
      },
    },
  );
  context.subscriptions.push(vscode.commands.registerCommand("still.commands.restart", async () => {
    await client?.stop();
    await client?.start();
  }));
  await client.start();
}
export function deactivate(): Thenable<void> | undefined {
  if (client !== null) {
    return client.stop()
  }
  return undefined;
}

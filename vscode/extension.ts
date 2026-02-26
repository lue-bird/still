import * as vscode from "vscode";
import * as language_client_node from "vscode-languageclient/node";
import * as child_process from "node:child_process";

// switch to your locally built debug executable path when developing
const languageServerExecutableName: string = "lily";
let client: language_client_node.LanguageClient | null = null;
export async function activate(context: vscode.ExtensionContext): Promise<void> {
  client = new language_client_node.LanguageClient(
    "lily",
    "lily",
    async () => child_process.spawn(languageServerExecutableName),
    {
      diagnosticCollectionName: "lily",
      documentSelector: [{ scheme: "file", language: "lily" }],
      synchronize: {
        fileEvents: vscode.workspace.createFileSystemWatcher("**/*.lily")
      },
    },
  );
  context.subscriptions.push(vscode.commands.registerCommand("lily.commands.restart", async () => {
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

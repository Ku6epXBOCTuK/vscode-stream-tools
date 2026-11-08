import * as vscode from "vscode";

const VIEW_ID = "stream-tools.unread";

const empty: vscode.TreeDataProvider<vscode.TreeItem> = {
	getChildren: () => [],
	getTreeItem: (item) => item,
};

export class UnreadBadge {
	private readonly tree = vscode.window.createTreeView(VIEW_ID, {
		treeDataProvider: empty,
	});

	set(unread: number): void {
		this.tree.badge =
			unread > 0
				? {
						value: unread,
						tooltip: `${unread} unread ${unread === 1 ? "message" : "messages"}`,
					}
				: undefined;
	}

	dispose(): void {
		this.tree.dispose();
	}
}

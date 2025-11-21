import { createConnection } from 'vscode-languageserver/node';
import { startServer } from '@verter/language-server/dist/server';
import type { Connection } from 'vscode-languageserver';
import * as path from 'node:path';
import { URI } from 'vscode-uri';
import { TextDocument } from 'vscode-languageserver-textdocument';
import { StreamMessageReader, StreamMessageWriter } from 'vscode-languageserver/node';
import { Duplex } from 'stream';

let connection: Connection | undefined;
let documents: Map<string, TextDocument> = new Map();

export const testWorkspacePath = path.resolve(__dirname, '../../test-workspace');

export interface VerterServer {
	connection: Connection;
	openDocument: (uri: string, languageId: string, content: string) => Promise<TextDocument>;
	closeDocument: (uri: string) => Promise<void>;
	sendCompletionRequest: (uri: string, position: { line: number; character: number }) => Promise<any>;
	sendHoverRequest: (uri: string, position: { line: number; character: number }) => Promise<any>;
	sendDefinitionRequest: (uri: string, position: { line: number; character: number }) => Promise<any>;
}

export async function getVerterServer(): Promise<VerterServer> {
	if (!connection) {
		// Create duplex streams for client-server communication
		const up = new Duplex({
			write(chunk, _encoding, callback) {
				setImmediate(() => down.push(chunk));
				callback();
			},
			read() {}
		});

		const down = new Duplex({
			write(chunk, _encoding, callback) {
				setImmediate(() => up.push(chunk));
				callback();
			},
			read() {}
		});

		// Create server connection using streams
		const serverConnection = createConnection(
			new StreamMessageReader(up),
			new StreamMessageWriter(down)
		);

		// Start the server with our connection
		startServer({ connection: serverConnection });

		// Create client connection
		const clientConnection = createConnection(
			new StreamMessageReader(down),
			new StreamMessageWriter(up)
		);

		clientConnection.listen();

		// Initialize the server
		await clientConnection.sendRequest('initialize', {
			processId: process.pid,
			rootUri: URI.file(testWorkspacePath).toString(),
			capabilities: {
				textDocument: {
					completion: {
						completionItem: {
							snippetSupport: true,
							labelDetailsSupport: true,
						}
					},
					hover: {
						contentFormat: ['markdown', 'plaintext']
					}
				},
				workspace: {
					workspaceFolders: true,
					configuration: true,
				}
			},
			workspaceFolders: [
				{
					uri: URI.file(testWorkspacePath).toString(),
					name: 'test-workspace'
				}
			]
		});

		await clientConnection.sendNotification('initialized', {});

		// Give the server a moment to fully initialize
		await new Promise(resolve => setTimeout(resolve, 100));

		// Replace the connection with clientConnection for sending requests
		connection = clientConnection;
	}

	return {
		connection,
		openDocument: async (uri: string, languageId: string, content: string) => {
			const document = TextDocument.create(uri, languageId, 1, content);
			documents.set(uri, document);

			await connection!.sendNotification('textDocument/didOpen', {
				textDocument: {
					uri,
					languageId,
					version: 1,
					text: content
				}
			});

			return document;
		},
		closeDocument: async (uri: string) => {
			documents.delete(uri);
			await connection!.sendNotification('textDocument/didClose', {
				textDocument: { uri }
			});
		},
		sendCompletionRequest: async (uri: string, position: { line: number; character: number }) => {
			return await connection!.sendRequest('textDocument/completion', {
				textDocument: { uri },
				position
			});
		},
		sendHoverRequest: async (uri: string, position: { line: number; character: number }) => {
			return await connection!.sendRequest('textDocument/hover', {
				textDocument: { uri },
				position
			});
		},
		sendDefinitionRequest: async (uri: string, position: { line: number; character: number }) => {
			return await connection!.sendRequest('textDocument/definition', {
				textDocument: { uri },
				position
			});
		}
	};
}

export async function closeVerterServer() {
	if (connection) {
		try {
			await connection.sendRequest('shutdown', {});
			await connection.sendNotification('exit', {});
		} catch (e) {
			// Ignore errors during shutdown
		}
		connection.dispose();
		connection = undefined;
		documents.clear();
	}
}

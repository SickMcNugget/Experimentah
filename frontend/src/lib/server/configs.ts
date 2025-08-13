import fs from 'node:fs/promises';
import path from 'node:path';

export type ConfigType = 'infrastructure' | 'experiment';

export type ConfigInfo = {
	name: string;
	content: string;
	modifiedDate: string; // ISO string
};

// TODO(jackson): use environment to configure paths?
export function getConfigDir(configType: ConfigType): string {
	const base = path.resolve(process.cwd(), 'storage', 'configs');
	const subdir = configType === 'infrastructure' ? 'infrastructure' : 'experiments';
	return path.join(base, subdir);
}

// Creates the directory if it doesn't exist
export async function ensureConfigDir(configType: ConfigType): Promise<string> {
	const dir = getConfigDir(configType);
	await fs.mkdir(dir, { recursive: true });
	return dir;
}

export function sanitizeFilename(filename: string): string {
	// Prevent path traversal
	const base = path.basename(filename);
	// Prevent hidden files and directories, and collapse illegal sequences
	// TODO(jackson): add more checks or use something to ensure valid name?
	if (base === '' || base === '.' || base === '..') {
		throw new Error('Invalid filename');
	}

	return base;
}

export async function listConfigs(configType: ConfigType): Promise<ConfigInfo[]> {
	const dir = await ensureConfigDir(configType);
	const entries = await fs.readdir(dir, { withFileTypes: true });
	const files = entries.filter((e) => e.isFile());
	const result: ConfigInfo[] = [];
	for (const file of files) {
		const full = path.join(dir, file.name);
		const [stat, content] = await Promise.all([fs.stat(full), fs.readFile(full, 'utf8')]);
		result.push({ name: file.name, content, modifiedDate: stat.mtime.toISOString() });
	}
	// Sort by modified to make things easier
	result.sort((a, b) => (a.modifiedDate < b.modifiedDate ? 1 : -1));
	return result;
}

export async function saveConfig(
	configType: ConfigType,
	fileName: string,
	fileContent: ArrayBuffer | Buffer | Uint8Array
): Promise<ConfigInfo> {
	const dir = await ensureConfigDir(configType);
	const safeName = sanitizeFilename(fileName);
	const full = path.join(dir, safeName);
	let buffer: Buffer;
	if (fileContent instanceof Uint8Array && !(fileContent instanceof Buffer)) {
		buffer = Buffer.from(fileContent);
	} else if (fileContent instanceof ArrayBuffer) {
		buffer = Buffer.from(new Uint8Array(fileContent));
	} else {
		buffer = fileContent as Buffer;
	}
	await fs.writeFile(full, buffer);
	const [stat, content] = await Promise.all([fs.stat(full), fs.readFile(full, 'utf8')]);
	return { name: safeName, content, modifiedDate: stat.mtime.toISOString() };
}

export async function deleteConfig(configType: ConfigType, fileName: string): Promise<void> {
	const dir = await ensureConfigDir(configType);
	const safeName = sanitizeFilename(fileName);
	const full = path.join(dir, safeName);

	// Ensure the resolved path is within the intended directory so other files can't be deleted
	const resolved = path.resolve(full);
	if (!resolved.startsWith(path.resolve(dir) + path.sep)) {
		throw new Error('Invalid path');
	}
	await fs.unlink(full);
}




import type { RequestHandler } from '@sveltejs/kit';
import { deleteConfig, listConfigs, saveConfig } from '$lib/server/configs';

export const GET: RequestHandler = async () => {
	const list = await listConfigs('experiment');
	return new Response(JSON.stringify(list), { status: 200, headers: { 'content-type': 'application/json' } });
};

export const POST: RequestHandler = async ({ request }) => {
	const form = await request.formData();
	const file = form.get('file');
	if (!(file instanceof File)) return new Response('file is required', { status: 400 });
	const arrayBuffer = await file.arrayBuffer();
	const saved = await saveConfig('experiment', file.name, arrayBuffer);
	return new Response(JSON.stringify(saved), { status: 201, headers: { 'content-type': 'application/json' } });
};

export const DELETE: RequestHandler = async ({ url }) => {
	const name = url.searchParams.get('name');
	if (!name) return new Response('name is required', { status: 400 });
	await deleteConfig('experiment', name);
	return new Response(null, { status: 204 });
};




/**
 * multipart/form-data parsing for upload endpoints. Uses the Workers runtime's
 * native `Request.formData()` rather than a library (writing-lessons rule #16:
 * simple requirement + simple existing solution + no expansion expected).
 *
 * Upload envelopes contain exactly two binary parts: `bundle` (.omni zip) and
 * `thumbnail` (WebP). Any missing or non-file part is a `MultipartError` that
 * route handlers map to Malformed/BadRequest per worker-api.md §3.
 */

export interface MultipartParts {
  bundle: Uint8Array;
  thumbnail: Uint8Array;
}

export class MultipartError extends Error {
  readonly part: string;
  constructor(part: string, message: string) {
    super(message);
    this.name = 'MultipartError';
    this.part = part;
  }
}

async function readPart(form: FormData, name: string): Promise<Uint8Array> {
  const v = form.get(name);
  if (v === null) {
    throw new MultipartError(name, `missing multipart part: ${name}`);
  }
  if (typeof v === 'string') {
    throw new MultipartError(name, `multipart part ${name} must be a file, got string`);
  }
  // `v` is a File/Blob in Workers runtime.
  const buf = await (v as Blob).arrayBuffer();
  return new Uint8Array(buf);
}

export async function parseMultipart(req: Request): Promise<MultipartParts> {
  let form: FormData;
  try {
    form = await req.formData();
  } catch (e) {
    throw new MultipartError(
      '_envelope',
      `failed to parse multipart body: ${(e as Error).message}`,
    );
  }
  const bundle = await readPart(form, 'bundle');
  const thumbnail = await readPart(form, 'thumbnail');
  return { bundle, thumbnail };
}

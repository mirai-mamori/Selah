export interface FilePayload {
  fileName: string;
  fileBase64: string;
}

export async function fileToPayload(file: File): Promise<FilePayload> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let i = 0; i < bytes.length; i += 8192) {
    const chunk = bytes.subarray(i, i + 8192);
    for (let j = 0; j < chunk.length; j += 1) binary += String.fromCharCode(chunk[j]);
  }
  return { fileName: file.name, fileBase64: btoa(binary) };
}

export async function filesToPayload(files: File[]): Promise<FilePayload[]> {
  const payloads: FilePayload[] = [];
  for (const file of files) payloads.push(await fileToPayload(file));
  return payloads;
}

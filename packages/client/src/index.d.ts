export interface SessionKeyPair {
  secretKey: Uint8Array;
  publicKey: Uint8Array;
}

export interface SiteResponse {
  status: number;
  content_type?: string;
  headers: { name: string; value: string }[];
  body: Uint8Array;
}

export declare class SiteClient {
  static generateSessionKey(): Uint8Array;
  static sessionPublicKey(secretKey: Uint8Array): Uint8Array;
  static connect(invitationUrl: string, nowUnix: number): Promise<SiteClient>;
  static connectMinted(
    token: string,
    secretKey: Uint8Array,
    nowUnix: number,
  ): Promise<SiteClient>;
  static resume(
    sessionGrant: string,
    secretKey: Uint8Array,
    nowUnix: number,
    expectedOrigin: string,
  ): Promise<SiteClient>;
  readonly entryPath: string;
  readonly siteId: string;
  readonly resumeCredential: string;
  exportResumeKey(): Uint8Array;
  fetch(
    method: string,
    path: string,
    headers: { name: string; value: string }[],
    body: Uint8Array,
  ): Promise<SiteResponse>;
  openSocket(path: string): Promise<unknown>;
  close(): void;
}

export function ready(wasmUrl?: RequestInfo | URL): Promise<void>;
export function generateSessionKey(): SessionKeyPair;
export function connect(invitationUrl: string): Promise<SiteClient>;
export function connectMinted(
  token: string,
  secretKey: Uint8Array,
): Promise<SiteClient>;
export function resume(
  sessionGrant: string,
  secretKey: Uint8Array,
  expectedOrigin: string,
): Promise<SiteClient>;

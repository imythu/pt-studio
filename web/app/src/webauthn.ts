// WebAuthn 浏览器端：服务端选项中的二进制字段为 base64url，提交时同样编码（identity webauthn 模块）。

type Json = Record<string, unknown>;

function toBuf(s: string): ArrayBuffer {
  const b64 = s.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (s.length % 4)) % 4);
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out.buffer;
}

function toB64(b: ArrayBuffer | null): string | null {
  if (!b) return null;
  let s = '';
  for (const x of new Uint8Array(b)) s += String.fromCharCode(x);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export const webauthnSupported = (): boolean => typeof window !== 'undefined' && 'PublicKeyCredential' in window;

const ids = (list: unknown) => ((list as { id: string; type: string }[] | undefined) ?? []).map((c) => ({ ...c, id: toBuf(c.id) }));

/** navigator.credentials.create，返回可提交的 JSON */
export async function createCredential(pk: Json): Promise<Json> {
  const user = pk.user as Json;
  const cred = (await navigator.credentials.create({
    publicKey: {
      ...(pk as unknown as PublicKeyCredentialCreationOptions),
      challenge: toBuf(pk.challenge as string),
      user: { ...(user as unknown as PublicKeyCredentialUserEntity), id: toBuf(user.id as string) },
      excludeCredentials: ids(pk.excludeCredentials) as PublicKeyCredentialDescriptor[],
    },
  })) as PublicKeyCredential;
  const r = cred.response as AuthenticatorAttestationResponse;
  return {
    id: cred.id,
    rawId: toB64(cred.rawId),
    type: cred.type,
    response: { clientDataJSON: toB64(r.clientDataJSON), attestationObject: toB64(r.attestationObject), transports: r.getTransports?.() ?? [] },
    clientExtensionResults: cred.getClientExtensionResults(),
  };
}

/** navigator.credentials.get，返回可提交的断言 JSON */
export async function getAssertion(pk: Json): Promise<Json> {
  const cred = (await navigator.credentials.get({
    publicKey: {
      ...(pk as unknown as PublicKeyCredentialRequestOptions),
      challenge: toBuf(pk.challenge as string),
      allowCredentials: ids(pk.allowCredentials) as PublicKeyCredentialDescriptor[],
    },
  })) as PublicKeyCredential;
  const r = cred.response as AuthenticatorAssertionResponse;
  return {
    id: cred.id,
    rawId: toB64(cred.rawId),
    type: cred.type,
    response: {
      clientDataJSON: toB64(r.clientDataJSON),
      authenticatorData: toB64(r.authenticatorData),
      signature: toB64(r.signature),
      userHandle: toB64(r.userHandle),
    },
  };
}

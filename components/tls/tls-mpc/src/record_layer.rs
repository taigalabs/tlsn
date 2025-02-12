use mpz_garble::value::ValueRef;

use tls_core::{
    cipher::make_tls12_aad,
    msgs::{
        base::Payload,
        enums::{ContentType, ProtocolVersion},
        message::{OpaqueMessage, PlainMessage},
    },
};

use crate::{error::Kind, MpcTlsError};

pub(crate) struct Encrypter {
    aead: Box<dyn aead::Aead>,
    seq: u64,
    sent_bytes: usize,
    transcript_id: String,
    opaque_transcript_id: String,
}

impl Encrypter {
    pub(crate) fn new(
        aead: Box<dyn aead::Aead>,
        transcript_id: String,
        opaque_transcript_id: String,
    ) -> Self {
        Self {
            aead,
            seq: 0,
            sent_bytes: 0,
            transcript_id,
            opaque_transcript_id,
        }
    }

    /// Returns the number of application data bytes encrypted
    pub(crate) fn sent_bytes(&self) -> usize {
        self.sent_bytes
    }

    pub(crate) async fn set_key(&mut self, key: ValueRef, iv: ValueRef) -> Result<(), MpcTlsError> {
        self.aead.set_key(key, iv).await.map_err(|e| {
            MpcTlsError::new_with_source(Kind::Encrypt, "error setting encryption key", e)
        })?;

        Ok(())
    }

    pub(crate) async fn encrypt_private(
        &mut self,
        msg: PlainMessage,
    ) -> Result<OpaqueMessage, MpcTlsError> {
        let PlainMessage {
            typ,
            version,
            payload,
        } = msg;

        self.prepare_encrypt(typ);

        let seq = self.seq;
        let len = payload.0.len();
        let explicit_nonce = seq.to_be_bytes().to_vec();
        let aad = make_tls12_aad(seq, typ, version, len);

        let ciphertext = self
            .aead
            .encrypt_private(explicit_nonce.clone(), payload.0, aad.to_vec())
            .await
            .map_err(|e| MpcTlsError::new_with_source(Kind::Encrypt, "encrypt_private error", e))?;

        self.record_message(typ, len);

        let mut payload = explicit_nonce;
        payload.extend(ciphertext);

        Ok(OpaqueMessage {
            typ,
            version,
            payload: Payload::new(payload),
        })
    }

    pub(crate) async fn encrypt_blind(
        &mut self,
        typ: ContentType,
        version: ProtocolVersion,
        len: usize,
    ) -> Result<(), MpcTlsError> {
        self.prepare_encrypt(typ);

        let seq = self.seq;
        let explicit_nonce = seq.to_be_bytes().to_vec();
        let aad = make_tls12_aad(seq, typ, version, len);

        self.aead
            .encrypt_blind(explicit_nonce, len, aad.to_vec())
            .await
            .map_err(|e| MpcTlsError::new_with_source(Kind::Encrypt, "encrypt_blind error", e))?;

        self.record_message(typ, len);

        Ok(())
    }

    pub(crate) async fn encrypt_public(
        &mut self,
        msg: PlainMessage,
    ) -> Result<OpaqueMessage, MpcTlsError> {
        let PlainMessage {
            typ,
            version,
            payload,
        } = msg;

        self.prepare_encrypt(typ);

        let seq = self.seq;
        let len = payload.0.len();
        let explicit_nonce = seq.to_be_bytes().to_vec();
        let aad = make_tls12_aad(seq, typ, version, len);

        let ciphertext = self
            .aead
            .encrypt_public(explicit_nonce.clone(), payload.0, aad.to_vec())
            .await
            .map_err(|e| MpcTlsError::new_with_source(Kind::Encrypt, "encrypt_public error", e))?;

        self.record_message(typ, len);

        let mut payload = explicit_nonce;
        payload.extend(ciphertext);

        Ok(OpaqueMessage {
            typ,
            version,
            payload: Payload::new(payload),
        })
    }

    fn prepare_encrypt(&mut self, typ: ContentType) {
        // Set the transcript id depending on the type of message
        match typ {
            ContentType::ApplicationData => {
                self.aead.set_transcript_id(&self.transcript_id);
            }
            _ => self.aead.set_transcript_id(&self.opaque_transcript_id),
        }
    }

    fn record_message(&mut self, typ: ContentType, len: usize) {
        self.seq += 1;
        if let ContentType::ApplicationData = typ {
            self.sent_bytes += len;
        }
    }
}

pub(crate) struct Decrypter {
    aead: Box<dyn aead::Aead>,
    seq: u64,
    recv_bytes: usize,
    transcript_id: String,
    opaque_transcript_id: String,
}

impl Decrypter {
    pub(crate) fn new(
        aead: Box<dyn aead::Aead>,
        transcript_id: String,
        opaque_transcript_id: String,
    ) -> Self {
        Self {
            aead,
            seq: 0,
            recv_bytes: 0,
            transcript_id,
            opaque_transcript_id,
        }
    }

    /// Returns the number of application data bytes decrypted
    pub(crate) fn recv_bytes(&self) -> usize {
        self.recv_bytes
    }

    pub(crate) async fn set_key(&mut self, key: ValueRef, iv: ValueRef) -> Result<(), MpcTlsError> {
        self.aead.set_key(key, iv).await.map_err(|e| {
            MpcTlsError::new_with_source(Kind::Decrypt, "error setting decryption key", e)
        })?;

        Ok(())
    }

    pub(crate) async fn decrypt_private(
        &mut self,
        msg: OpaqueMessage,
    ) -> Result<PlainMessage, MpcTlsError> {
        let OpaqueMessage {
            typ,
            version,
            mut payload,
        } = msg;

        let explicit_nonce: Vec<u8> = payload.0.drain(..8).collect();
        let len = payload.0.len() - 16;
        let seq = self.seq;

        self.prepare_decrypt(typ);

        let aad = make_tls12_aad(seq, typ, version, len);
        let plaintext = self
            .aead
            .decrypt_private(explicit_nonce, payload.0, aad.to_vec())
            .await
            .map_err(|e| MpcTlsError::new_with_source(Kind::Decrypt, "decrypt_private error", e))?;

        self.record_message(typ, len);

        Ok(PlainMessage {
            typ,
            version,
            payload: Payload::new(plaintext),
        })
    }

    pub(crate) async fn decrypt_blind(&mut self, msg: OpaqueMessage) -> Result<(), MpcTlsError> {
        let OpaqueMessage {
            typ,
            version,
            mut payload,
        } = msg;

        let explicit_nonce: Vec<u8> = payload.0.drain(..8).collect();
        let len = payload.0.len() - 16;
        let seq = self.seq;

        self.prepare_decrypt(typ);

        let aad = make_tls12_aad(seq, typ, version, len);
        self.aead
            .decrypt_blind(explicit_nonce, payload.0, aad.to_vec())
            .await
            .map_err(|e| MpcTlsError::new_with_source(Kind::Decrypt, "decrypt_blind error", e))?;

        self.record_message(typ, len);

        Ok(())
    }

    pub(crate) async fn decrypt_public(
        &mut self,
        msg: OpaqueMessage,
    ) -> Result<PlainMessage, MpcTlsError> {
        let OpaqueMessage {
            typ,
            version,
            mut payload,
        } = msg;

        let explicit_nonce: Vec<u8> = payload.0.drain(..8).collect();
        let len = payload.0.len() - 16;
        let seq = self.seq;

        self.prepare_decrypt(typ);

        let aad = make_tls12_aad(seq, typ, version, len);
        let plaintext = self
            .aead
            .decrypt_public(explicit_nonce, payload.0, aad.to_vec())
            .await
            .map_err(|e| MpcTlsError::new_with_source(Kind::Decrypt, "decrypt_public error", e))?;

        self.record_message(typ, len);

        Ok(PlainMessage {
            typ,
            version,
            payload: Payload::new(plaintext),
        })
    }

    fn prepare_decrypt(&mut self, typ: ContentType) {
        // Set the transcript id depending on the type of message
        match typ {
            ContentType::ApplicationData => {
                self.aead.set_transcript_id(&self.transcript_id);
            }
            _ => self.aead.set_transcript_id(&self.opaque_transcript_id),
        }
    }

    fn record_message(&mut self, typ: ContentType, len: usize) {
        self.seq += 1;
        if let ContentType::ApplicationData = typ {
            self.recv_bytes += len;
        }
    }
}

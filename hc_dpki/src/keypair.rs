use crate::{
    bundle,
    holochain_sodium::{aead, kx, random::random_secbuf, secbuf::SecBuf, sign},
    util,
};
use holochain_core_types::{agent::KeyBuffer, error::HolochainError};
use rustc_serialize::json;
use std::str;

pub struct Keypair {
    pub pub_keys: String,
    pub sign_priv: SecBuf,
    pub enc_priv: SecBuf,
}

pub const SEEDSIZE: usize = 32 as usize;

impl Keypair {
    /// derive the pairs from a 32 byte seed buffer
    ///  
    /// @param {SecBuf} seed - the seed buffer
    pub fn new_from_seed(seed: &mut SecBuf) -> Result<Self, HolochainError> {
        let mut seed = seed;
        let mut sign_public_key = SecBuf::with_insecure(sign::PUBLICKEYBYTES);
        let mut sign_secret_key = SecBuf::with_secure(sign::SECRETKEYBYTES);
        let mut enc_public_key = SecBuf::with_insecure(kx::PUBLICKEYBYTES);
        let mut enc_secret_key = SecBuf::with_secure(kx::SECRETKEYBYTES);

        sign::seed_keypair(&mut sign_public_key, &mut sign_secret_key, &mut seed)?;
        kx::seed_keypair(&mut seed, &mut enc_public_key, &mut enc_secret_key)?;

        Ok(Keypair {
            pub_keys: util::encode_id(&mut sign_public_key, &mut enc_public_key),
            sign_priv: sign_secret_key,
            enc_priv: enc_secret_key,
        })
    }

    /// get the keypair identifier string
    ///
    /// @return {string}
    pub fn get_id(&mut self) -> String {
        return self.pub_keys.clone();
    }

    /// generate an encrypted persistence bundle
    ///
    /// @param {SecBuf} passphrase - the encryption passphrase
    ///
    /// @param {string} hint - additional info / description for the bundle
    pub fn get_bundle(
        &mut self,
        passphrase: &mut SecBuf,
        hint: String,
    ) -> Result<bundle::KeyBundle, HolochainError> {
        let mut passphrase = passphrase;
        let bundle_type: String = "hcKeypair".to_string();
        let kk = KeyBuffer::with_corrected(&self.pub_keys)?;
        let sk = kk.get_sig() as &[u8];
        let ek = kk.get_enc() as &[u8];
        let mut sk_buf = SecBuf::with_insecure(32);
        let mut ek_buf = SecBuf::with_insecure(32);
        util::convert_array_to_secbuf(&sk, &mut sk_buf);
        util::convert_array_to_secbuf(&ek, &mut ek_buf);

        // Merge all the secbuf together before encoding
        let mut sign_pub = sk.to_vec();
        let mut enc_pub = ek.to_vec();
        let mut sign_priv = self.sign_priv.read_lock().to_vec();
        let mut enc_priv = self.enc_priv.read_lock().to_vec();
   
        sign_pub.append(&mut enc_pub);
        sign_pub.append(&mut sign_priv);
        sign_pub.append(&mut enc_priv);
        let mut key_buf = SecBuf::with_insecure(sign_pub.len());
        util::convert_vec_to_secbuf(&sign_pub,&mut key_buf);

        let pw_enc: bundle::ReturnBundleData = util::pw_enc(&mut key_buf, &mut passphrase)?;
        let bundle_data_serialized = json::encode(&pw_enc).unwrap();

        // conver to base64
        let bundle_data_encoded = base64::encode(&bundle_data_serialized);

        Ok(bundle::KeyBundle {
            bundle_type,
            hint,
            data: bundle_data_encoded,
        })
    }

    /// initialize the pairs from an encrypted persistence bundle
    ///
    /// @param {object} bundle - persistence info
    ///
    /// @param {SecBuf} passphrase - decryption passphrase
    pub fn from_bundle(
        bundle: &bundle::KeyBundle,
        passphrase: &mut SecBuf,
    ) -> Result<Keypair, HolochainError> {
        // decoding the bundle.data of type util::ReturnBundledata
        let bundle_decoded = base64::decode(&bundle.data)?;
        let bundle_string = str::from_utf8(&bundle_decoded).unwrap();
        let data: bundle::ReturnBundleData = json::decode(&bundle_string).unwrap();
        let mut keys_salt = util::pw_dec(&data, passphrase)?;
        let key_buf = keys_salt.read_lock();
        let mut sign_priv = SecBuf::with_secure(64);
        let mut enc_priv = SecBuf::with_secure(32);
        util::convert_array_to_secbuf(&key_buf[64..128],&mut sign_priv);
        util::convert_array_to_secbuf(&key_buf[128..160],&mut enc_priv);

        let sp = &key_buf[0..32];
        let ep = &key_buf[32..64];
        Ok(Keypair {
            pub_keys: KeyBuffer::with_raw_parts(array_ref![sp, 0, 32], array_ref![ep, 0, 32]).render(),
            enc_priv,
            sign_priv,
        })
    }

    /// sign some arbitrary data with the signing private key
    ///
    /// @param {SecBuf} data - the data to sign
    ///
    /// @param {SecBuf} signature - Empty Buf the sign
    pub fn sign(
        &mut self,
        data: &mut SecBuf,
        signature: &mut SecBuf,
    ) -> Result<(), HolochainError> {
        let mut data = data;
        let mut signature = signature;
        let mut sign_priv = &mut self.sign_priv;
        sign::sign(&mut data, &mut sign_priv, &mut signature)?;
        Ok(())
    }

    /// verify data that was signed with our private signing key
    ///
    /// @param {SecBuf} signature
    ///
    /// @param {SecBuf} data
    pub fn verify(
        &mut self,
        signature: &mut SecBuf,
        data: &mut SecBuf,
    ) -> Result<i32, HolochainError> {
        let mut data = data;
        let mut signature = signature;
        let pub_keys = &mut self.pub_keys;
        let mut sign_pub = SecBuf::with_insecure(sign::PUBLICKEYBYTES);
        let mut enc_pub = SecBuf::with_insecure(kx::PUBLICKEYBYTES);

        util::decode_id(pub_keys.clone(), &mut sign_pub, &mut enc_pub)?;
        let v: i32 = sign::verify(&mut signature, &mut data, &mut sign_pub);
        Ok(v)
    }

    /// encrypt arbitrary data to be readale by potentially multiple recipients
    ///
    /// @param {array<string>} recipientIds - multiple recipient identifier strings
    ///
    /// @param {Buffer} data - the data to encrypt
    ///
    /// @param {Buffer} out - Empty vec[secBuf]
    pub fn encrypt(
        &mut self,
        recipient_id: Vec<&String>,
        data: &mut SecBuf,
        out: &mut Vec<SecBuf>,
    ) -> Result<(), HolochainError> {
        let mut sym_secret = SecBuf::with_secure(32);
        random_secbuf(&mut sym_secret);

        let mut srv_rx = SecBuf::with_insecure(kx::SESSIONKEYBYTES);
        let mut srv_tx = SecBuf::with_insecure(kx::SESSIONKEYBYTES);

        let pub_keys = &mut self.pub_keys;
        let mut sign_pub = SecBuf::with_insecure(sign::PUBLICKEYBYTES);
        let mut enc_pub = SecBuf::with_insecure(kx::PUBLICKEYBYTES);
        util::decode_id(pub_keys.to_string(), &mut sign_pub, &mut enc_pub)?;

        let mut enc_priv = &mut self.enc_priv;

        for client_pk in recipient_id {
            let mut r_sign_pub = SecBuf::with_insecure(sign::PUBLICKEYBYTES);
            let mut r_enc_pub = SecBuf::with_insecure(kx::PUBLICKEYBYTES);

            util::decode_id(client_pk.to_string(), &mut r_sign_pub, &mut r_enc_pub)?;

            kx::server_session(
                &mut enc_pub,
                &mut enc_priv,
                &mut r_enc_pub,
                &mut srv_rx,
                &mut srv_tx,
            )?;

            let mut nonce = SecBuf::with_insecure(16);
            random_secbuf(&mut nonce);
            let mut cipher = SecBuf::with_insecure(sym_secret.len() + aead::ABYTES);

            aead::enc(&mut sym_secret, &mut srv_tx, None, &mut nonce, &mut cipher)?;
            out.push(nonce);
            out.push(cipher);
        }

        let mut nonce = SecBuf::with_insecure(16);
        random_secbuf(&mut nonce);
        let mut cipher = SecBuf::with_insecure(data.len() + aead::ABYTES);
        let mut data = data;
        aead::enc(&mut data, &mut sym_secret, None, &mut nonce, &mut cipher)?;
        out.push(nonce);
        out.push(cipher);
        Ok(())
    }

    /// attempt to decrypt the cipher buffer (assuming it was targeting us)
    ///
    /// @param {string} sourceId - identifier string of who encrypted this data
    ///
    /// @param {Buffer} cipher - the encrypted data
    ///
    /// @return {Result<SecBuf,String>} - the decrypted data
    pub fn decrypt(
        &mut self,
        source_id: String,
        cipher_bundle: &mut Vec<SecBuf>,
    ) -> Result<SecBuf, HolochainError> {
        let mut source_sign_pub = SecBuf::with_insecure(sign::PUBLICKEYBYTES);
        let mut source_enc_pub = SecBuf::with_insecure(kx::PUBLICKEYBYTES);
        util::decode_id(source_id, &mut source_sign_pub, &mut source_enc_pub)?;

        let client_pub_keys = &self.pub_keys;
        let mut client_sign_pub = SecBuf::with_insecure(sign::PUBLICKEYBYTES);
        let mut client_enc_pub = SecBuf::with_insecure(kx::PUBLICKEYBYTES);
        util::decode_id(
            client_pub_keys.to_string(),
            &mut client_sign_pub,
            &mut client_enc_pub,
        )?;
        let mut client_enc_priv = &mut self.enc_priv;

        let mut cli_rx = SecBuf::with_insecure(kx::SESSIONKEYBYTES);
        let mut cli_tx = SecBuf::with_insecure(kx::SESSIONKEYBYTES);
        kx::client_session(
            &mut client_enc_pub,
            &mut client_enc_priv,
            &mut source_enc_pub,
            &mut cli_rx,
            &mut cli_tx,
        )?;

        let mut sys_secret_check: Option<SecBuf> = None;

        while cipher_bundle.len() != 2 {
            println!("Round trip");
            let mut n: Vec<_> = cipher_bundle.splice(..1, vec![]).collect();
            let mut c: Vec<_> = cipher_bundle.splice(..1, vec![]).collect();
            let mut n = &mut n[0];
            let mut c = &mut c[0];
            let mut sys_secret = SecBuf::with_insecure(c.len() - aead::ABYTES);

            match aead::dec(&mut sys_secret, &mut cli_rx, None, &mut n, &mut c) {
                Ok(_) => {
                    if util::check_if_wrong_secbuf(&mut sys_secret) {
                        println!("TRUE");
                        sys_secret_check = Some(sys_secret);
                        break;
                    } else {
                        println!("FALSE");

                        sys_secret_check = None;
                    }
                }
                Err(_) => {
                    sys_secret_check = None;
                }
            };
        }

        let mut c: Vec<_> = cipher_bundle
            .splice(cipher_bundle.len() - 1.., vec![])
            .collect();
        let mut n: Vec<_> = cipher_bundle
            .splice(cipher_bundle.len() - 1.., vec![])
            .collect();
        let mut n = &mut n[0];
        let mut c = &mut c[0];
        let mut dm = SecBuf::with_insecure(c.len() - aead::ABYTES);

        if let Some(mut secret) = sys_secret_check {
            aead::dec(&mut dm, &mut secret, None, &mut n, &mut c)?;
            Ok(dm)
        } else {
            Err(HolochainError::new(
                &"could not decrypt - not a recipient?".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holochain_sodium::random::random_secbuf;

    #[test]
    fn it_should_set_keypair_from_seed() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);

        let keypair = Keypair::new_from_seed(&mut seed).unwrap();

        // let pub_keys = keypair.pub_keys.read_lock();
        // println!("{:?}",pub_keys);
        // let sign_priv = keypair.sign_priv.read_lock();
        // println!("{:?}",sign_priv);
        // let enc_priv = keypair.enc_priv.read_lock();
        // println!("{:?}",enc_priv);

        assert_eq!(64, keypair.sign_priv.len());
        assert_eq!(32, keypair.enc_priv.len());
    }

    #[test]
    fn it_should_get_id() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);
        let mut keypair = Keypair::new_from_seed(&mut seed).unwrap();

        let pk: String = keypair.get_id();
        println!("pk: {:?}", pk);
        let pk1: String = keypair.get_id();
        println!("pk1: {:?}", pk1);
        assert_eq!(pk, pk1);
    }

    #[test]
    fn it_should_sign_message_and_verify() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);
        let mut keypair = Keypair::new_from_seed(&mut seed).unwrap();

        let mut message = SecBuf::with_insecure(16);
        random_secbuf(&mut message);

        let mut message_signed = SecBuf::with_insecure(64);

        keypair.sign(&mut message, &mut message_signed).unwrap();

        let check: i32 = keypair.verify(&mut message_signed, &mut message).unwrap();
        assert_eq!(0, check);
    }

    #[test]
    fn it_should_encode_n_decode_data() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);
        let mut keypair_main = Keypair::new_from_seed(&mut seed).unwrap();

        let mut seed_1 = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed_1);
        let mut keypair_1 = Keypair::new_from_seed(&mut seed_1).unwrap();

        let mut message = SecBuf::with_insecure(16);
        random_secbuf(&mut message);

        let recipient_id = vec![&keypair_1.pub_keys];

        let mut out = Vec::new();
        keypair_main
            .encrypt(recipient_id, &mut message, &mut out)
            .unwrap();

        match keypair_1.decrypt(keypair_main.pub_keys, &mut out) {
            Ok(mut dm) => {
                let message = message.read_lock();
                let dm = dm.read_lock();
                assert_eq!(format!("{:?}", *message), format!("{:?}", *dm));
            }
            Err(_) => {
                assert!(false);
            }
        };
    }

    #[test]
    fn it_should_encode_n_decode_data_for_multiple_users2() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);
        let mut keypair_main = Keypair::new_from_seed(&mut seed).unwrap();

        let mut seed_1 = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed_1);
        let keypair_1 = Keypair::new_from_seed(&mut seed_1).unwrap();

        let mut seed_2 = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed_2);
        let mut keypair_2 = Keypair::new_from_seed(&mut seed_2).unwrap();

        let mut message = SecBuf::with_insecure(16);
        random_secbuf(&mut message);

        let recipient_id = vec![&keypair_1.pub_keys, &keypair_2.pub_keys];

        let mut out = Vec::new();
        keypair_main
            .encrypt(recipient_id, &mut message, &mut out)
            .unwrap();

        match keypair_2.decrypt(keypair_main.pub_keys, &mut out) {
            Ok(mut dm) => {
                let message = message.read_lock();
                let dm = dm.read_lock();
                assert_eq!(format!("{:?}", *message), format!("{:?}", *dm));
            }
            Err(_) => {
                assert!(false);
            }
        };
    }

    #[test]
    fn it_should_encode_n_decode_data_for_multiple_users1() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);
        let mut keypair_main = Keypair::new_from_seed(&mut seed).unwrap();

        let mut seed_1 = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed_1);
        let mut keypair_1 = Keypair::new_from_seed(&mut seed_1).unwrap();

        let mut seed_2 = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed_2);
        let keypair_2 = Keypair::new_from_seed(&mut seed_2).unwrap();

        let mut message = SecBuf::with_insecure(16);
        random_secbuf(&mut message);

        let recipient_id = vec![&keypair_1.pub_keys, &keypair_2.pub_keys];

        let mut out = Vec::new();
        keypair_main
            .encrypt(recipient_id, &mut message, &mut out)
            .unwrap();

        match keypair_1.decrypt(keypair_main.pub_keys, &mut out) {
            Ok(mut dm) => {
                println!("Decrypted Message: {:?}", dm);
                let message = message.read_lock();
                let dm = dm.read_lock();
                assert_eq!(format!("{:?}", *message), format!("{:?}", *dm));
            }
            Err(_) => {
                println!("Error");
                assert!(false);
            }
        };
    }

    #[test]
    fn it_should_with_fail_when_wrong_key_used_to_decrypt() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);
        let mut keypair_main = Keypair::new_from_seed(&mut seed).unwrap();

        let mut seed_1 = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed_1);
        let keypair_1 = Keypair::new_from_seed(&mut seed_1).unwrap();

        let mut seed_2 = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed_2);
        let mut keypair_2 = Keypair::new_from_seed(&mut seed_2).unwrap();

        let mut message = SecBuf::with_insecure(16);
        random_secbuf(&mut message);

        let recipient_id = vec![&keypair_1.pub_keys];

        let mut out = Vec::new();
        keypair_main
            .encrypt(recipient_id, &mut message, &mut out)
            .unwrap();

        keypair_2
            .decrypt(keypair_main.pub_keys, &mut out)
            .expect_err("should have failed");
    }

    #[test]
    fn it_should_get_from_bundle() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);
        let mut keypair = Keypair::new_from_seed(&mut seed).unwrap();
        let mut passphrase = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut passphrase);

        let bundle: bundle::KeyBundle = keypair
            .get_bundle(&mut passphrase, "hint".to_string())
            .unwrap();

        let keypair_from_bundle = Keypair::from_bundle(&bundle, &mut passphrase).unwrap();

        assert_eq!(64, keypair_from_bundle.sign_priv.len());
        assert_eq!(32, keypair_from_bundle.enc_priv.len());
        assert_eq!(92, keypair_from_bundle.pub_keys.len());
    }

    #[test]
    fn it_should_get_bundle() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);
        let mut keypair = Keypair::new_from_seed(&mut seed).unwrap();
        let mut passphrase = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut passphrase);

        let bundle: bundle::KeyBundle = keypair
            .get_bundle(&mut passphrase, "hint".to_string())
            .unwrap();

        println!("Bundle.bundle_type: {}", bundle.bundle_type);
        println!("Bundle.Hint: {}", bundle.hint);
        println!("Bundle.data: {}", bundle.data);

        assert_eq!("hint", bundle.hint);
    }
    
    #[test]
    fn it_should_try_get_bundle_and_decode_it() {
        let mut seed = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut seed);
        let mut keypair = Keypair::new_from_seed(&mut seed).unwrap();
        let mut passphrase = SecBuf::with_insecure(SEEDSIZE);
        random_secbuf(&mut passphrase);

        let bundle: bundle::KeyBundle = keypair
            .get_bundle(&mut passphrase, "hint".to_string())
            .unwrap();

        println!("Bundle.bundle_type: {}", bundle.bundle_type);
        println!("Bundle.Hint: {}", bundle.hint);
        println!("Bundle.data: {}", bundle.data);

        let keypair_from_bundle = Keypair::from_bundle(&bundle, &mut passphrase).unwrap();

        assert_eq!(64, keypair_from_bundle.sign_priv.len());
        assert_eq!(32, keypair_from_bundle.enc_priv.len());
        assert_eq!(92, keypair_from_bundle.pub_keys.len());
    }
}

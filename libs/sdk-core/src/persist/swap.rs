use rusqlite::{named_params, OptionalExtension, Params, Row, Transaction, TransactionBehavior};

use crate::models::{OpeningFeeParams, SwapInfo, SwapStatus};

use super::{
    db::{SqliteStorage, StringArray},
    error::PersistError,
    error::PersistResult,
};

#[derive(Debug, Clone)]
pub(crate) struct SwapChainInfo {
    pub(crate) unconfirmed_sats: u64,
    pub(crate) unconfirmed_tx_ids: Vec<String>,
    pub(crate) confirmed_sats: u64,
    pub(crate) confirmed_tx_ids: Vec<String>,
    pub(crate) confirmed_at: Option<u32>,
    pub(crate) confirmed_at_timestamp: Option<u64>,
    pub(crate) total_incoming_txs: u64,
}

impl SqliteStorage {
    pub(crate) fn insert_swap(&self, swap_info: SwapInfo) -> PersistResult<()> {
        let mut con = self.get_connection()?;
        let tx = con.transaction_with_behavior(TransactionBehavior::Immediate)?;

        tx.execute("
         INSERT INTO sync.swaps (
           bitcoin_address, 
           created_at, 
           lock_height, 
           payment_hash, 
           preimage, 
           private_key, 
           public_key, 
           swapper_public_key, 
           script,
           min_allowed_deposit, 
           max_allowed_deposit,
           max_swapper_payable
         )
         VALUES (:bitcoin_address, :created_at, :lock_height, :payment_hash, :preimage, :private_key, :public_key, :swapper_public_key, :script, :min_allowed_deposit, :max_allowed_deposit, :max_swapper_payable)",
         named_params! {
             ":bitcoin_address": swap_info.bitcoin_address,
             ":created_at": swap_info.created_at,
             ":lock_height": swap_info.lock_height,
             ":payment_hash": swap_info.payment_hash,
             ":preimage": swap_info.preimage,
             ":private_key": swap_info.private_key,
             ":public_key": swap_info.public_key,
             ":swapper_public_key": swap_info.swapper_public_key,            
             ":script": swap_info.script,             
             ":min_allowed_deposit": swap_info.min_allowed_deposit,
             ":max_allowed_deposit": swap_info.max_allowed_deposit,
             ":max_swapper_payable": swap_info.max_swapper_payable,
         },
        )?;

        tx.execute(
            "
        INSERT INTO swaps_info (
          bitcoin_address, 
          status,
          bolt11,
          paid_msat, 
          unconfirmed_sats, 
          unconfirmed_tx_ids, 
          confirmed_sats,
          confirmed_tx_ids,
          confirmed_at,
          confirmed_at_timestamp,
          total_incoming_txs
        ) VALUES (:bitcoin_address, :status, :bolt11, :paid_msat, :unconfirmed_sats, :unconfirmed_tx_ids, :confirmed_sats, :confirmed_tx_ids, :confirmed_at, :confirmed_at_timestamp, :total_incoming_txs)",
            named_params! {
               ":bitcoin_address": swap_info.bitcoin_address,
               ":status": swap_info.status as i32,
               ":bolt11": None::<String>,
               ":paid_msat": swap_info.paid_msat,
               ":unconfirmed_sats": swap_info.unconfirmed_sats,
               ":unconfirmed_tx_ids": StringArray(swap_info.unconfirmed_tx_ids),
               ":confirmed_sats": swap_info.confirmed_sats,
               ":confirmed_tx_ids": StringArray(swap_info.confirmed_tx_ids),
               ":confirmed_at": swap_info.confirmed_at,
               ":confirmed_at_timestamp": swap_info.confirmed_at_timestamp,
               ":total_incoming_txs": swap_info.total_incoming_txs,
            },
        )?;

        Self::insert_swaps_fees(
            &tx,
            swap_info.bitcoin_address,
            swap_info.channel_opening_fees.ok_or_else(|| {
                PersistError::generic("Dynamic fees must be set when creating a new swap")
            })?,
        )?;

        tx.commit()?;
        Ok(())
    }

    pub(crate) fn update_swap_paid_amount(
        &self,
        bitcoin_address: String,
        paid_msat: u64,
        status: SwapStatus,
    ) -> PersistResult<()> {
        self.get_connection()?.execute(
            "UPDATE swaps_info SET paid_msat=:paid_msat, status=:status where bitcoin_address=:bitcoin_address",
            named_params! {
             ":paid_msat": paid_msat,
             ":bitcoin_address": bitcoin_address,
             ":status": status as u32,
            },
        )?;
        Ok(())
    }

    pub(crate) fn update_swap_max_allowed_deposit(
        &self,
        bitcoin_address: String,
        max_allowed_deposit: i64,
    ) -> PersistResult<()> {
        self.get_connection()?.execute(
            "UPDATE sync.swaps SET max_allowed_deposit=:max_allowed_deposit where bitcoin_address=:bitcoin_address",
            named_params! {
             ":max_allowed_deposit": max_allowed_deposit,
             ":bitcoin_address": bitcoin_address,
            },
        )?;

        Ok(())
    }

    pub(crate) fn update_swap_redeem_error(
        &self,
        bitcoin_address: String,
        redeem_err: String,
    ) -> PersistResult<()> {
        self.get_connection()?.execute(
            "UPDATE swaps_info SET last_redeem_error=:redeem_err where bitcoin_address=:bitcoin_address",
            named_params! {
             ":redeem_err": redeem_err,
             ":bitcoin_address": bitcoin_address,
            },
        )?;

        Ok(())
    }

    pub(crate) fn update_swap_bolt11(
        &self,
        bitcoin_address: String,
        bolt11: String,
    ) -> PersistResult<()> {
        self.get_connection()?.execute(
            "UPDATE swaps_info SET bolt11=:bolt11 where bitcoin_address=:bitcoin_address",
            named_params! {
             ":bolt11": bolt11,
             ":bitcoin_address": bitcoin_address,
            },
        )?;

        Ok(())
    }

    fn insert_swaps_fees(
        tx: &Transaction,
        bitcoin_address: String,
        channel_opening_fees: OpeningFeeParams,
    ) -> PersistResult<()> {
        tx.execute(
            "INSERT OR REPLACE INTO sync.swaps_fees (bitcoin_address, created_at, channel_opening_fees) VALUES(:bitcoin_address, CURRENT_TIMESTAMP, :channel_opening_fees)",
            named_params! {
             ":bitcoin_address": bitcoin_address,
             ":channel_opening_fees": channel_opening_fees,
            },
        )?;

        Ok(())
    }

    /// Update the dynamic fees associated with a swap
    pub(crate) fn update_swap_fees(
        &self,
        bitcoin_address: String,
        channel_opening_fees: OpeningFeeParams,
    ) -> PersistResult<()> {
        let mut con = self.get_connection()?;
        let tx = con.transaction_with_behavior(TransactionBehavior::Immediate)?;

        Self::insert_swaps_fees(&tx, bitcoin_address, channel_opening_fees)?;

        tx.commit()?;
        Ok(())
    }

    pub(crate) fn insert_swap_refund_tx_ids(
        &self,
        bitcoin_address: String,
        refund_tx_id: String,
    ) -> PersistResult<()> {
        self.get_connection()?.execute(
            "INSERT INTO sync.swap_refunds (bitcoin_address, refund_tx_id) VALUES(:bitcoin_address, :refund_tx_id)",
            named_params! {
             ":bitcoin_address": bitcoin_address,
             ":refund_tx_id": refund_tx_id,
            },
        )?;

        Ok(())
    }

    pub(crate) fn update_swap_chain_info(
        &self,
        bitcoin_address: String,
        chain_info: SwapChainInfo,
        status: SwapStatus,
    ) -> PersistResult<SwapInfo> {
        self.get_connection()?.execute(
            "UPDATE swaps_info SET total_incoming_txs=:total_incoming_txs, unconfirmed_sats=:unconfirmed_sats, unconfirmed_tx_ids=:unconfirmed_tx_ids, confirmed_sats=:confirmed_sats, confirmed_tx_ids=:confirmed_tx_ids, status=:status, confirmed_at=:confirmed_at, confirmed_at_timestamp=:confirmed_at_timestamp where bitcoin_address=:bitcoin_address",
            named_params! {
             ":unconfirmed_sats": chain_info.unconfirmed_sats,
             ":unconfirmed_tx_ids": StringArray(chain_info.unconfirmed_tx_ids),
             ":confirmed_sats": chain_info.confirmed_sats,
             ":bitcoin_address": bitcoin_address,             
             ":confirmed_tx_ids": StringArray(chain_info.confirmed_tx_ids),
             ":status": status as u32,
             ":confirmed_at": chain_info.confirmed_at,
             ":confirmed_at_timestamp": chain_info.confirmed_at_timestamp,
             ":total_incoming_txs": chain_info.total_incoming_txs,
            },
        )?;
        Ok(self.get_swap_info_by_address(bitcoin_address)?.unwrap())
    }
    //(SELECT json_group_array(value) FROM json_each(json_group_array(refund_tx_id)) WHERE refund_tx_id is not null) as refund_tx_ids,
    pub(crate) fn select_swap_query(&self, where_clause: &str, prefix: &str) -> String {
        let swap_fields = format!("        
          swaps.bitcoin_address  as {prefix}bitcoin_address,
          swaps.created_at as {prefix}created_at,
          lock_height as {prefix}lock_height,
          payment_hash as {prefix}payment_hash,
          preimage as {prefix}preimage,
          private_key as {prefix}private_key,
          public_key as {prefix}public_key,
          swapper_public_key as {prefix}swapper_public_key,
          script as {prefix}script,
          min_allowed_deposit as {prefix}min_allowed_deposit,
          max_allowed_deposit as {prefix}max_allowed_deposit,
          max_swapper_payable as {prefix}max_swapper_payable,
          bolt11 as {prefix}bolt11,
          paid_msat as {prefix}paid_msat,
          unconfirmed_sats as {prefix}unconfirmed_sats,
          confirmed_sats as {prefix}confirmed_sats,
          total_incoming_txs as {prefix}total_incoming_txs,
          status as {prefix}status,             
          (SELECT json_group_array(refund_tx_id) FROM sync.swap_refunds as swap_refunds where bitcoin_address = swaps.bitcoin_address) as {prefix}refund_tx_ids,
          unconfirmed_tx_ids as {prefix}unconfirmed_tx_ids,
          confirmed_tx_ids as {prefix}confirmed_tx_ids,
          last_redeem_error as {prefix}last_redeem_error,
          swaps_fees.channel_opening_fees as {prefix}channel_opening_fees,
          swaps_info.confirmed_at as {prefix}confirmed_at,
          swaps_info.confirmed_at_timestamp as {prefix}confirmed_at_timestamp         
        ");

        format!(
            "
            SELECT
             {swap_fields}
            FROM sync.swaps as swaps
             LEFT JOIN swaps_info ON swaps.bitcoin_address = swaps_info.bitcoin_address
             LEFT JOIN sync.swaps_fees as swaps_fees ON swaps.bitcoin_address = swaps_fees.bitcoin_address
             LEFT JOIN sync.swap_refunds as swap_refunds ON swaps.bitcoin_address = swap_refunds.bitcoin_address
            WHERE {}
            ",
            where_clause
        )
    }

    pub(crate) fn select_swap_fields(&self, prefix: &str) -> String {
        format!(
            "        
          {prefix}bitcoin_address,
          {prefix}created_at,
          {prefix}lock_height,
          {prefix}payment_hash,
          {prefix}preimage,
          {prefix}private_key,
          {prefix}public_key,
          {prefix}swapper_public_key,
          {prefix}script,
          {prefix}min_allowed_deposit,
          {prefix}max_allowed_deposit,
          {prefix}max_swapper_payable,
          {prefix}bolt11,
          {prefix}paid_msat,
          {prefix}unconfirmed_sats,
          {prefix}confirmed_sats,
          {prefix}total_incoming_txs,
          {prefix}status,             
          {prefix}refund_tx_ids,
          {prefix}unconfirmed_tx_ids,
          {prefix}confirmed_tx_ids,
          {prefix}last_redeem_error,
          {prefix}channel_opening_fees,
          {prefix}confirmed_at,
          {prefix}confirmed_at_timestamp          
          "
        )
    }

    fn select_single_swap<P>(
        &self,
        where_clause: &str,
        params: P,
    ) -> PersistResult<Option<SwapInfo>>
    where
        P: Params,
    {
        Ok(self
            .get_connection()?
            .query_row(&self.select_swap_query(where_clause, ""), params, |row| {
                self.sql_row_to_swap(row, "")
            })
            .optional()?)
    }

    pub(crate) fn get_swap_info_by_hash(&self, hash: &Vec<u8>) -> PersistResult<Option<SwapInfo>> {
        self.select_single_swap("payment_hash = ?1", [hash])
    }

    pub(crate) fn get_swap_info_by_address(
        &self,
        address: String,
    ) -> PersistResult<Option<SwapInfo>> {
        self.select_single_swap("swaps.bitcoin_address = ?1", [address])
    }

    pub(crate) fn list_swaps_with_status(
        &self,
        status: SwapStatus,
    ) -> PersistResult<Vec<SwapInfo>> {
        let con = self.get_connection()?;
        let mut stmt = con.prepare(&self.select_swap_query("status = ?1", ""))?;

        let vec: Vec<SwapInfo> = stmt
            .query_map([status as u32], |row| self.sql_row_to_swap(row, ""))?
            .map(|i| i.unwrap())
            .collect();

        Ok(vec)
    }

    pub(crate) fn list_swaps(&self) -> PersistResult<Vec<SwapInfo>> {
        let con = self.get_connection()?;
        let mut stmt = con.prepare(&self.select_swap_query("true", ""))?;

        let vec: Vec<SwapInfo> = stmt
            .query_map([], |row| self.sql_row_to_swap(row, ""))?
            .map(|i| i.unwrap())
            .collect();

        Ok(vec)
    }

    pub(crate) fn sql_row_to_swap(
        &self,
        row: &Row,
        prefix: &str,
    ) -> PersistResult<SwapInfo, rusqlite::Error> {
        let status: i32 = row
            .get::<&str, Option<i32>>(format!("{prefix}status").as_str())?
            .unwrap_or(SwapStatus::Initial as i32);
        let status: SwapStatus = status.try_into().unwrap_or(SwapStatus::Initial);
        let refund_txs_raw: String = row
            .get::<&str, Option<String>>(format!("{prefix}refund_tx_ids").as_str())?
            .unwrap_or("[]".to_string());
        let refund_tx_ids: Vec<String> = serde_json::from_str(refund_txs_raw.as_str()).unwrap();
        // let t: Vec<String> =
        //     serde_json::from_value(refund_txs_raw).map_err(|e| FromSqlError::InvalidType)?;

        let unconfirmed_tx_ids: StringArray = row
            .get::<&str, Option<StringArray>>(format!("{prefix}unconfirmed_tx_ids").as_str())?
            .unwrap_or(StringArray(vec![]));
        let confirmed_txs_raw: StringArray = row
            .get::<&str, Option<StringArray>>(format!("{prefix}confirmed_tx_ids").as_str())?
            .unwrap_or(StringArray(vec![]));
        let bitcoin_address = row.get(format!("{prefix}bitcoin_address").as_str())?;
        Ok(SwapInfo {
            bitcoin_address,
            created_at: row.get(format!("{prefix}created_at").as_str())?,
            lock_height: row.get(format!("{prefix}lock_height").as_str())?,
            payment_hash: row.get(format!("{prefix}payment_hash").as_str())?,
            preimage: row.get(format!("{prefix}preimage").as_str())?,
            private_key: row.get(format!("{prefix}private_key").as_str())?,
            public_key: row.get(format!("{prefix}public_key").as_str())?,
            swapper_public_key: row.get(format!("{prefix}swapper_public_key").as_str())?,
            script: row.get(format!("{prefix}script").as_str())?,
            bolt11: row.get(format!("{prefix}bolt11").as_str())?,
            paid_msat: row
                .get::<&str, Option<u64>>(format!("{prefix}paid_msat").as_str())?
                .unwrap_or_default(),
            unconfirmed_sats: row
                .get::<&str, Option<u64>>(format!("{prefix}unconfirmed_sats").as_str())?
                .unwrap_or_default(),
            confirmed_sats: row
                .get::<&str, Option<u64>>(format!("{prefix}confirmed_sats").as_str())?
                .unwrap_or_default(),
            total_incoming_txs: row
                .get::<&str, Option<u64>>(format!("{prefix}total_incoming_txs").as_str())?
                .unwrap_or_default(),
            status,
            refund_tx_ids,
            unconfirmed_tx_ids: unconfirmed_tx_ids.0,
            confirmed_tx_ids: confirmed_txs_raw.0,
            min_allowed_deposit: row.get(format!("{prefix}min_allowed_deposit").as_str())?,
            max_allowed_deposit: row.get(format!("{prefix}max_allowed_deposit").as_str())?,
            max_swapper_payable: row.get(format!("{prefix}max_swapper_payable").as_str())?,
            last_redeem_error: row.get(format!("{prefix}last_redeem_error").as_str())?,
            channel_opening_fees: row.get(format!("{prefix}channel_opening_fees").as_str())?,
            confirmed_at: row.get(format!("{prefix}confirmed_at").as_str())?,
            confirmed_at_timestamp: row.get(format!("{prefix}confirmed_at_timestamp").as_str())?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::persist::db::SqliteStorage;
    use crate::persist::error::PersistResult;
    use crate::persist::swap::SwapChainInfo;
    use crate::test_utils::get_test_ofp_48h;
    use crate::{OpeningFeeParams, SwapInfo, SwapStatus};
    use rusqlite::{named_params, Connection};

    #[test]
    fn test_swaps() -> PersistResult<(), Box<dyn std::error::Error>> {
        use crate::persist::test_utils;
        fn list_in_progress_swaps(storage: &SqliteStorage) -> PersistResult<Vec<SwapInfo>> {
            Ok(storage
                .list_swaps()?
                .into_iter()
                .filter(SwapInfo::in_progress)
                .collect())
        }

        let storage = SqliteStorage::new(test_utils::create_test_sql_dir());

        storage.init()?;
        let tested_swap_info = SwapInfo {
            bitcoin_address: String::from("1"),
            created_at: 0,
            lock_height: 100,
            payment_hash: vec![1],
            preimage: vec![2],
            private_key: vec![3],
            public_key: vec![4],
            swapper_public_key: vec![5],
            script: vec![5],
            bolt11: None,
            paid_msat: 0,
            unconfirmed_sats: 0,
            confirmed_sats: 0,
            total_incoming_txs: 0,
            status: SwapStatus::Initial,
            refund_tx_ids: Vec::new(),
            unconfirmed_tx_ids: Vec::new(),
            confirmed_tx_ids: Vec::new(),
            min_allowed_deposit: 0,
            max_allowed_deposit: 100,
            max_swapper_payable: 200,
            last_redeem_error: None,
            channel_opening_fees: Some(get_test_ofp_48h(1, 1).into()),
            confirmed_at: None,
            confirmed_at_timestamp: None,
        };
        storage.insert_swap(tested_swap_info.clone())?;
        let item_value = storage.get_swap_info_by_address("1".to_string())?.unwrap();
        assert_eq!(item_value, tested_swap_info);

        let in_progress = list_in_progress_swaps(&storage)?;
        assert_eq!(in_progress.len(), 0);

        let non_existent_swap = storage.get_swap_info_by_address("non-existent".to_string())?;
        assert!(non_existent_swap.is_none());

        let empty_swaps = storage.list_swaps_with_status(SwapStatus::Refundable)?;
        assert_eq!(empty_swaps.len(), 0);

        let swaps = storage.list_swaps_with_status(SwapStatus::Initial)?;
        assert_eq!(swaps.len(), 1);

        let err = storage.insert_swap(tested_swap_info.clone());
        //assert_eq!(swaps.len(), 1);
        assert!(err.is_err());

        let chain_info = SwapChainInfo {
            unconfirmed_sats: 20,
            unconfirmed_tx_ids: vec![String::from("333"), String::from("444")],
            confirmed_sats: 0,
            confirmed_tx_ids: vec![],
            confirmed_at: None,
            confirmed_at_timestamp: None,
            total_incoming_txs: 0,
        };

        let swap_after_chain_update = storage.update_swap_chain_info(
            tested_swap_info.bitcoin_address.clone(),
            chain_info.clone(),
            tested_swap_info
                .with_chain_info(chain_info.clone(), 0)
                .status,
        )?;
        let in_progress = list_in_progress_swaps(&storage)?;
        assert_eq!(in_progress[0], swap_after_chain_update);

        let chain_info = SwapChainInfo {
            unconfirmed_sats: 0,
            unconfirmed_tx_ids: vec![],
            confirmed_sats: 20,
            confirmed_tx_ids: vec![String::from("333"), String::from("444")],
            confirmed_at: Some(1000),
            confirmed_at_timestamp: Some(1000),
            total_incoming_txs: 1,
        };
        let swap_after_chain_update = storage.update_swap_chain_info(
            tested_swap_info.bitcoin_address.clone(),
            chain_info.clone(),
            tested_swap_info.with_chain_info(chain_info, 1001).status,
        )?;
        let in_progress = list_in_progress_swaps(&storage)?;
        assert_eq!(in_progress[0], swap_after_chain_update);

        let chain_info = SwapChainInfo {
            unconfirmed_sats: 0,
            unconfirmed_tx_ids: vec![],
            confirmed_sats: 20,
            confirmed_tx_ids: vec![String::from("333"), String::from("444")],
            confirmed_at: Some(1000),
            confirmed_at_timestamp: Some(1000),
            total_incoming_txs: 1,
        };
        storage.update_swap_chain_info(
            tested_swap_info.bitcoin_address.clone(),
            chain_info.clone(),
            tested_swap_info.with_chain_info(chain_info, 10000).status,
        )?;
        storage.insert_swap_refund_tx_ids(
            tested_swap_info.bitcoin_address.clone(),
            String::from("111"),
        )?;
        storage.insert_swap_refund_tx_ids(
            tested_swap_info.bitcoin_address.clone(),
            String::from("222"),
        )?;
        let in_progress = list_in_progress_swaps(&storage)?;
        assert_eq!(in_progress.len(), 0);

        storage.update_swap_redeem_error(
            tested_swap_info.bitcoin_address.clone(),
            String::from("test error"),
        )?;
        let updated_swap = storage
            .get_swap_info_by_address(tested_swap_info.bitcoin_address.clone())?
            .unwrap();
        assert_eq!(
            updated_swap.last_redeem_error.clone().unwrap(),
            String::from("test error")
        );

        storage.update_swap_bolt11(tested_swap_info.bitcoin_address.clone(), "bolt11".into())?;
        storage.update_swap_paid_amount(
            tested_swap_info.bitcoin_address.clone(),
            30_000,
            updated_swap.with_paid_amount(30_000, 10000).status,
        )?;
        let updated_swap = storage
            .get_swap_info_by_address(tested_swap_info.bitcoin_address.clone())?
            .unwrap();
        assert_eq!(updated_swap.bolt11.unwrap(), "bolt11".to_string());
        assert_eq!(updated_swap.paid_msat, 30_000);
        assert_eq!(updated_swap.confirmed_sats, 20);
        assert_eq!(
            updated_swap.refund_tx_ids,
            vec![String::from("111"), String::from("222")]
        );
        assert_eq!(
            updated_swap.confirmed_tx_ids,
            vec![String::from("333"), String::from("444")]
        );
        assert_eq!(updated_swap.status, SwapStatus::Completed);

        let chain_info = SwapChainInfo {
            unconfirmed_sats: 0,
            unconfirmed_tx_ids: vec![],
            confirmed_sats: 20,
            confirmed_tx_ids: vec![String::from("333"), String::from("444")],
            confirmed_at: Some(1000),
            confirmed_at_timestamp: Some(1000),
            total_incoming_txs: 2,
        };
        storage.update_swap_chain_info(
            tested_swap_info.bitcoin_address.clone(),
            chain_info.clone(),
            tested_swap_info.with_chain_info(chain_info, 10000).status,
        )?;
        let updated_swap = storage
            .get_swap_info_by_address(tested_swap_info.bitcoin_address)?
            .unwrap();
        assert_eq!(updated_swap.status, SwapStatus::Refundable);
        Ok(())
    }

    #[test]
    /// Checks if an empty column is converted to None
    fn test_rusqlite_empty_col_handling() -> PersistResult<()> {
        let db = Connection::open_in_memory()?;

        // Insert a NULL
        db.execute_batch("CREATE TABLE foo (fees_optional TEXT)")?;
        db.execute(
            "
         INSERT INTO foo ( fees_optional )
         VALUES ( NULL )",
            named_params! {},
        )?;

        // Read the column, expect None
        let res = db.query_row("SELECT fees_optional FROM foo", [], |row| {
            row.get::<usize, Option<OpeningFeeParams>>(0)
        })?;
        assert_eq!(res, None);

        Ok(())
    }
}

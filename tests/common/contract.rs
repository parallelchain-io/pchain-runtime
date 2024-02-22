use pchain_types::{
    blockchain::Command,
    cryptography::PublicAddress,
    runtime::{CallInput, DeployInput, TransferInput},
};

#[derive(borsh::BorshSerialize, borsh::BorshDeserialize)]
pub struct CallResult;

impl CallResult {
    pub fn parse<T: borsh::BorshDeserialize>(return_value: Vec<u8>) -> Option<T> {
        if return_value.is_empty() {
            return None;
        }
        let mut return_bs = return_value.as_slice();
        T::deserialize(&mut return_bs).map_or(None, |value| Some(value))
    }
}

pub struct ArgsBuilder {
    pub args: Option<Vec<Vec<u8>>>,
}

impl ArgsBuilder {
    pub fn new() -> Self {
        Self { args: None }
    }

    pub fn empty_args(mut self) -> Self {
        self.args = Some(vec![]);
        self
    }

    pub fn add<T: borsh::BorshSerialize>(mut self, arg: T) -> Self {
        if self.args.is_none() {
            self.args = Some(vec![]);
        }

        if let Some(args) = &mut self.args {
            args.push(arg.try_to_vec().unwrap())
        }
        self
    }

    pub fn make_transfer(self, amount: u64, recipient: PublicAddress) -> Command {
        Command::Transfer(TransferInput { recipient, amount })
    }

    pub fn make_deploy(self, contract_code: Vec<u8>, cbi_version: u32) -> Command {
        Command::Deploy(DeployInput {
            contract: contract_code,
            cbi_version,
        })
    }

    pub fn make_call(
        self,
        amount: Option<u64>,
        target: PublicAddress,
        entry_name: &str,
    ) -> Command {
        Command::Call(CallInput {
            target,
            method: entry_name.to_string(),
            arguments: self.args,
            amount,
        })
    }
}
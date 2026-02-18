use anchor_client::solana_sdk::pubkey::Pubkey;
use anchor_client::solana_sdk::signature::{Keypair, Signer};
use anchor_client::solana_sdk::system_instruction;
use anchor_client::solana_client::rpc_client::RpcClient;
use anchor_client::{Client, Cluster};
use clap::{Args, Parser, Subcommand};
use solana_sdk::commitment_config::CommitmentConfig;
use std::rc::Rc;
use std::str::FromStr;

// Import the IDL (this will be generated after building)
// For now, we'll define the basic structure

#[derive(Parser)]
#[command(name = "escrow-cli")]
#[command(about = "A CLI for interacting with the Solana Escrow Engine")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// RPC URL for Solana cluster
    #[arg(long, default_value = "https://api.devnet.solana.com")]
    pub rpc_url: String,

    /// Path to keypair file
    #[arg(long, default_value = "~/.config/solana/id.json")]
    pub keypair: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new escrow
    Create(CreateArgs),
    /// Accept an existing escrow
    Accept(AcceptArgs),
    /// Confirm delivery and release funds
    Confirm(ConfirmArgs),
    /// Cancel an escrow before acceptance
    Cancel(CancelArgs),
    /// Resolve timeout for an escrow
    ResolveTimeout(ResolveTimeoutArgs),
    /// Get escrow details
    Info(InfoArgs),
}

#[derive(Args)]
pub struct CreateArgs {
    /// Token mint address
    #[arg(long)]
    pub mint: String,
    
    /// Amount of tokens to escrow
    #[arg(long)]
    pub amount: u64,
    
    /// Timeout duration in seconds
    #[arg(long, default_value = "86400")] // 24 hours default
    pub timeout: i64,
    
    /// Description of the escrow
    #[arg(long)]
    pub description: String,
}

#[derive(Args)]
pub struct AcceptArgs {
    /// Escrow account address
    #[arg(long)]
    pub escrow: String,
}

#[derive(Args)]
pub struct ConfirmArgs {
    /// Escrow account address
    #[arg(long)]
    pub escrow: String,
    
    /// Seller's token account address
    #[arg(long)]
    pub seller_token_account: String,
}

#[derive(Args)]
pub struct CancelArgs {
    /// Escrow account address
    #[arg(long)]
    pub escrow: String,
}

#[derive(Args)]
pub struct ResolveTimeoutArgs {
    /// Escrow account address
    #[arg(long)]
    pub escrow: String,
    
    /// Seller's token account address
    #[arg(long)]
    pub seller_token_account: String,
}

#[derive(Args)]
pub struct InfoArgs {
    /// Escrow account address
    #[arg(long)]
    pub escrow: String,
}

const PROGRAM_ID: &str = "6ChaRcWmP5YJg21Z6AL6B6zxG8vNPJfx2EZhwFJUPeKt";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    // Load keypair
    let keypair_path = shellexpand::tilde(&cli.keypair);
    let keypair_bytes = std::fs::read(&*keypair_path)?;
    let keypair = Keypair::from_bytes(&keypair_bytes)?;
    
    println!("Using wallet: {}", keypair.pubkey());
    println!("RPC URL: {}", cli.rpc_url);
    
    // Create client
    let client = Client::new_with_options(
        Cluster::Custom(cli.rpc_url, cli.rpc_url.clone()),
        Rc::new(keypair),
        CommitmentConfig::confirmed(),
    );
    
    let program = client.program(Pubkey::from_str(PROGRAM_ID)?)?;
    
    match cli.command {
        Commands::Create(args) => {
            println!("Creating escrow...");
            create_escrow(&program, args).await?;
        }
        Commands::Accept(args) => {
            println!("Accepting escrow...");
            accept_escrow(&program, args).await?;
        }
        Commands::Confirm(args) => {
            println!("Confirming delivery...");
            confirm_delivery(&program, args).await?;
        }
        Commands::Cancel(args) => {
            println!("Cancelling escrow...");
            cancel_escrow(&program, args).await?;
        }
        Commands::ResolveTimeout(args) => {
            println!("Resolving timeout...");
            resolve_timeout(&program, args).await?;
        }
        Commands::Info(args) => {
            println!("Getting escrow info...");
            get_escrow_info(&program, args).await?;
        }
    }
    
    Ok(())
}

async fn create_escrow(program: &anchor_client::Program<Rc<Keypair>>, args: CreateArgs) -> anyhow::Result<()> {
    let mint = Pubkey::from_str(&args.mint)?;
    let buyer = program.payer();
    
    // Derive escrow PDA
    let (escrow, _bump) = Pubkey::find_program_address(
        &[b"escrow", buyer.as_ref(), mint.as_ref()],
        &program.id(),
    );
    
    // Derive vault PDA
    let (vault_token_account, _vault_bump) = Pubkey::find_program_address(
        &[b"vault", escrow.as_ref()],
        &program.id(),
    );
    
    // Find buyer's token account (simplified - assumes ATA)
    let buyer_token_account = spl_associated_token_account::get_associated_token_address(
        &buyer,
        &mint,
    );
    
    println!("Escrow address: {}", escrow);
    println!("Vault address: {}", vault_token_account);
    println!("Creating escrow for {} tokens...", args.amount);
    
    let tx = program
        .request()
        .accounts(solana_escrow_engine::accounts::CreateEscrow {
            buyer,
            escrow,
            mint,
            buyer_token_account,
            vault_token_account,
            token_program: spl_token::ID,
            system_program: solana_sdk::system_program::ID,
            rent: solana_sdk::sysvar::rent::ID,
        })
        .args(solana_escrow_engine::instruction::CreateEscrow {
            amount: args.amount,
            timeout_duration: args.timeout,
            description: args.description,
        })
        .send()?;
    
    println!("Transaction signature: {}", tx);
    println!("Escrow created successfully!");
    
    Ok(())
}

async fn accept_escrow(program: &anchor_client::Program<Rc<Keypair>>, args: AcceptArgs) -> anyhow::Result<()> {
    let escrow = Pubkey::from_str(&args.escrow)?;
    let seller = program.payer();
    
    println!("Seller {} accepting escrow {}", seller, escrow);
    
    let tx = program
        .request()
        .accounts(solana_escrow_engine::accounts::AcceptEscrow {
            seller,
            escrow,
        })
        .args(solana_escrow_engine::instruction::AcceptEscrow {})
        .send()?;
    
    println!("Transaction signature: {}", tx);
    println!("Escrow accepted successfully!");
    
    Ok(())
}

async fn confirm_delivery(program: &anchor_client::Program<Rc<Keypair>>, args: ConfirmArgs) -> anyhow::Result<()> {
    let escrow = Pubkey::from_str(&args.escrow)?;
    let seller_token_account = Pubkey::from_str(&args.seller_token_account)?;
    let buyer = program.payer();
    
    // Derive vault PDA
    let (vault_token_account, _vault_bump) = Pubkey::find_program_address(
        &[b"vault", escrow.as_ref()],
        &program.id(),
    );
    
    let tx = program
        .request()
        .accounts(solana_escrow_engine::accounts::ConfirmDelivery {
            buyer,
            escrow,
            vault_token_account,
            seller_token_account,
            token_program: spl_token::ID,
        })
        .args(solana_escrow_engine::instruction::ConfirmDelivery {})
        .send()?;
    
    println!("Transaction signature: {}", tx);
    println!("Delivery confirmed, funds released!");
    
    Ok(())
}

async fn cancel_escrow(program: &anchor_client::Program<Rc<Keypair>>, args: CancelArgs) -> anyhow::Result<()> {
    let escrow = Pubkey::from_str(&args.escrow)?;
    let buyer = program.payer();
    
    // Get escrow data to find mint
    let escrow_data: solana_escrow_engine::Escrow = program.account(escrow)?;
    
    // Derive vault PDA
    let (vault_token_account, _vault_bump) = Pubkey::find_program_address(
        &[b"vault", escrow.as_ref()],
        &program.id(),
    );
    
    // Find buyer's token account (simplified - assumes ATA)
    let buyer_token_account = spl_associated_token_account::get_associated_token_address(
        &buyer,
        &escrow_data.mint,
    );
    
    let tx = program
        .request()
        .accounts(solana_escrow_engine::accounts::CancelEscrow {
            buyer,
            escrow,
            vault_token_account,
            buyer_token_account,
            token_program: spl_token::ID,
        })
        .args(solana_escrow_engine::instruction::CancelEscrow {})
        .send()?;
    
    println!("Transaction signature: {}", tx);
    println!("Escrow cancelled, funds returned!");
    
    Ok(())
}

async fn resolve_timeout(program: &anchor_client::Program<Rc<Keypair>>, args: ResolveTimeoutArgs) -> anyhow::Result<()> {
    let escrow = Pubkey::from_str(&args.escrow)?;
    let seller_token_account = Pubkey::from_str(&args.seller_token_account)?;
    let resolver = program.payer();
    
    // Derive vault PDA
    let (vault_token_account, _vault_bump) = Pubkey::find_program_address(
        &[b"vault", escrow.as_ref()],
        &program.id(),
    );
    
    let tx = program
        .request()
        .accounts(solana_escrow_engine::accounts::ResolveTimeout {
            resolver,
            escrow,
            vault_token_account,
            seller_token_account,
            token_program: spl_token::ID,
        })
        .args(solana_escrow_engine::instruction::ResolveTimeout {})
        .send()?;
    
    println!("Transaction signature: {}", tx);
    println!("Timeout resolved, funds released to seller!");
    
    Ok(())
}

async fn get_escrow_info(program: &anchor_client::Program<Rc<Keypair>>, args: InfoArgs) -> anyhow::Result<()> {
    let escrow = Pubkey::from_str(&args.escrow)?;
    
    let escrow_data: solana_escrow_engine::Escrow = program.account(escrow)?;
    
    println!("=== Escrow Information ===");
    println!("Address: {}", escrow);
    println!("Buyer: {}", escrow_data.buyer);
    println!("Seller: {}", escrow_data.seller);
    println!("Mint: {}", escrow_data.mint);
    println!("Amount: {}", escrow_data.amount);
    println!("State: {:?}", escrow_data.state);
    println!("Description: {}", escrow_data.description);
    println!("Created at: {}", escrow_data.created_at);
    println!("Timeout at: {}", escrow_data.timeout_at);
    
    if escrow_data.accepted_at > 0 {
        println!("Accepted at: {}", escrow_data.accepted_at);
    }
    
    if escrow_data.completed_at > 0 {
        println!("Completed at: {}", escrow_data.completed_at);
    }
    
    if escrow_data.cancelled_at > 0 {
        println!("Cancelled at: {}", escrow_data.cancelled_at);
    }
    
    Ok(())
}

// Placeholder module structure - this will be replaced by generated IDL
mod solana_escrow_engine {
    use anchor_lang::prelude::*;
    
    #[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
    pub struct Escrow {
        pub buyer: Pubkey,
        pub seller: Pubkey,
        pub mint: Pubkey,
        pub amount: u64,
        pub state: EscrowState,
        pub created_at: i64,
        pub accepted_at: i64,
        pub completed_at: i64,
        pub cancelled_at: i64,
        pub timeout_at: i64,
        pub description: String,
        pub bump: u8,
    }
    
    #[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
    pub enum EscrowState {
        Created,
        Accepted,
        Completed,
        Cancelled,
        TimedOut,
    }
    
    pub mod accounts {
        use super::*;
        
        #[derive(Accounts)]
        pub struct CreateEscrow {
            pub buyer: Pubkey,
            pub escrow: Pubkey,
            pub mint: Pubkey,
            pub buyer_token_account: Pubkey,
            pub vault_token_account: Pubkey,
            pub token_program: Pubkey,
            pub system_program: Pubkey,
            pub rent: Pubkey,
        }
        
        #[derive(Accounts)]
        pub struct AcceptEscrow {
            pub seller: Pubkey,
            pub escrow: Pubkey,
        }
        
        #[derive(Accounts)]
        pub struct ConfirmDelivery {
            pub buyer: Pubkey,
            pub escrow: Pubkey,
            pub vault_token_account: Pubkey,
            pub seller_token_account: Pubkey,
            pub token_program: Pubkey,
        }
        
        #[derive(Accounts)]
        pub struct CancelEscrow {
            pub buyer: Pubkey,
            pub escrow: Pubkey,
            pub vault_token_account: Pubkey,
            pub buyer_token_account: Pubkey,
            pub token_program: Pubkey,
        }
        
        #[derive(Accounts)]
        pub struct ResolveTimeout {
            pub resolver: Pubkey,
            pub escrow: Pubkey,
            pub vault_token_account: Pubkey,
            pub seller_token_account: Pubkey,
            pub token_program: Pubkey,
        }
    }
    
    pub mod instruction {
        use super::*;
        
        #[derive(AnchorSerialize, AnchorDeserialize)]
        pub struct CreateEscrow {
            pub amount: u64,
            pub timeout_duration: i64,
            pub description: String,
        }
        
        #[derive(AnchorSerialize, AnchorDeserialize)]
        pub struct AcceptEscrow {}
        
        #[derive(AnchorSerialize, AnchorDeserialize)]
        pub struct ConfirmDelivery {}
        
        #[derive(AnchorSerialize, AnchorDeserialize)]
        pub struct CancelEscrow {}
        
        #[derive(AnchorSerialize, AnchorDeserialize)]
        pub struct ResolveTimeout {}
    }
}
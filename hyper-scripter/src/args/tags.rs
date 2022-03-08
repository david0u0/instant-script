use crate::tag::TagSelector;
use serde::Serialize;
use structopt::clap::AppSettings::AllowLeadingHyphen;
use structopt::StructOpt;

#[derive(StructOpt, Debug, Serialize)]
pub struct Tags {
    #[structopt(subcommand)]
    pub subcmd: Option<TagsSubs>,
}

#[derive(StructOpt, Debug, Serialize)]
#[structopt(settings = &[AllowLeadingHyphen])]
pub enum TagsSubs {
    #[structopt(external_subcommand)]
    Other(Vec<String>),
    Unset {
        name: String,
    }, // TODO: new type?
    Set {
        #[structopt(long, short)]
        name: Option<String>,
        content: TagSelector,
    },
    LS {
        #[structopt(long, short)]
        known: bool,
        #[structopt(long, short, conflicts_with = "known")]
        named: bool,
    },
    Toggle {
        name: String,
    },
}

impl Tags {
    pub fn sanitize(&mut self) {
        match self.subcmd.as_ref() {
            None => {
                self.subcmd = Some(TagsSubs::LS {
                    named: false,
                    known: false,
                })
            }
            Some(TagsSubs::Other(args)) => {
                let args = ["tags", "set"]
                    .into_iter()
                    .chain(args.iter().map(|s| s.as_str()));
                self.subcmd = Some(TagsSubs::from_iter(args));
            }
            _ => (),
        }
    }
}

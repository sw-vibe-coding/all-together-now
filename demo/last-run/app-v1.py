import argparse

def greet_handler(args):
    print(f"Hello, {args.name}!")

def main():
    parser = argparse.ArgumentParser(description="CLI app with greet subcommand")
    subparsers = parser.add_subparsers(dest="command", help="Available commands")
    
    greet_parser = subparsers.add_parser("greet", help="Greet someone")
    greet_parser.add_argument("name", help="Name to greet")
    greet_parser.set_defaults(func=greet_handler)
    
    args = parser.parse_args()
    
    if args.command:
        args.func(args)
    else:
        parser.print_help()

if __name__ == "__main__":
    main()
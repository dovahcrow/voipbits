set dotenv-load := true

build:
  cross build --target x86_64-unknown-linux-gnu --release

  rm -rf target/lambda
  mkdir -p target/lambda
  cp --force target/x86_64-unknown-linux-gnu/release/bootstrap target/lambda/
  zip -j target/lambda/deploy.zip target/lambda/bootstrap

deploy:
  aws --region=us-west-2 --profile=personal lambda update-function-code --function-name VoipBits --zip-file fileb://target/lambda/deploy.zip

run:
  cargo run
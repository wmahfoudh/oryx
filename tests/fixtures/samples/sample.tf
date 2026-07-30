# Object storage for release artifacts.
variable "region" {
  type    = string
  default = "eu-west-1"
}

resource "aws_s3_bucket" "releases" {
  bucket = "oryx-releases"

  tags = {
    project = "oryx"
    keep    = true
  }
}

output "bucket_arn" {
  value = aws_s3_bucket.releases.arn
}
